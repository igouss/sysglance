use std::{
    io::Cursor,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use sysglance_core::reading::{Device, Unavailable, Value};

use super::{
    Event, Frame, GRANT, Helper, Line, Link, Registers, Reports, evolve, frames, parse, run,
};
use crate::sources::Source;

fn owned(command: &[&str]) -> Vec<String> {
    command.iter().map(|part| (*part).to_owned()).collect()
}

fn frame(registers: &[((u32, u32), u64)], reports: &[(&str, &str)]) -> Frame {
    Frame {
        registers: registers.iter().copied().collect(),
        reports: reports
            .iter()
            .map(|(drive, json)| ((*drive).to_owned(), (*json).to_owned()))
            .collect(),
    }
}

fn streaming(previous: Option<Registers>, latest: Registers, reports: Reports) -> Link {
    Link::Streaming {
        previous,
        latest,
        reports,
    }
}

fn helper(link: Link, registers_decodable: bool) -> Helper {
    Helper {
        link: Arc::new(Mutex::new(link)),
        command: "sudo -n helper".to_owned(),
        registers_decodable,
        sys: PathBuf::from("/nonexistent"),
    }
}

fn collected(link: Link, registers_decodable: bool) -> Vec<Device> {
    helper(link, registers_decodable).collect(Duration::from_secs(1))
}

#[test]
fn a_register_line_reads_cpu_address_and_value() {
    assert_eq!(
        parse("msr 8 0x3fe 0x595c8d4f1f62e"),
        Some(Line::Register {
            cpu: 8,
            address: 0x3fe,
            value: 0x5_95c8_d4f1_f62e,
        })
    );
}

#[test]
fn a_report_line_keeps_its_json_whole() {
    assert_eq!(
        parse("smart /dev/sda {\"a\": [1, 2]}"),
        Some(Line::Report {
            drive: "/dev/sda".to_owned(),
            json: "{\"a\": [1, 2]}".to_owned(),
        })
    );
}

#[test]
fn end_closes_a_frame() {
    assert_eq!(parse("end"), Some(Line::End));
}

#[test]
fn malformed_lines_are_ignored() {
    let lines = [
        "",
        "end now",
        "msr 0 0x10",
        "msr x 0x10 0x1",
        "msr 0 10 0x1",
        "msr 0 0x10 0x1 extra",
        "msr 0 0x100000000 0x1",
        "smart /dev/sda",
        "hello",
    ];
    let parsed: Vec<(&str, Option<Line>)> = lines.iter().map(|line| (*line, parse(line))).collect();
    let rejected: Vec<(&str, Option<Line>)> = lines.iter().map(|line| (*line, None)).collect();
    assert_eq!(parsed, rejected);
}

#[test]
fn the_first_frame_starts_the_stream() {
    let link = evolve(
        Link::Starting,
        Event::Read(frame(&[((0, 0x10), 1)], &[("/dev/sda", "{}")])),
    );
    assert_eq!(
        link,
        streaming(
            None,
            Registers::from([((0, 0x10), 1)]),
            Reports::from([("/dev/sda".to_owned(), "{}".to_owned())])
        )
    );
}

#[test]
fn a_later_frame_keeps_the_previous_registers_and_every_report() {
    let first = evolve(
        Link::Starting,
        Event::Read(frame(&[((0, 0x10), 1)], &[("/dev/sda", "old")])),
    );
    let second = evolve(
        first,
        Event::Read(frame(&[((0, 0x10), 2)], &[("/dev/nvme0", "new")])),
    );
    let third = evolve(
        second,
        Event::Read(frame(&[((0, 0x10), 3)], &[("/dev/sda", "newer")])),
    );
    assert_eq!(
        third,
        streaming(
            Some(Registers::from([((0, 0x10), 2)])),
            Registers::from([((0, 0x10), 3)]),
            Reports::from([
                ("/dev/nvme0".to_owned(), "new".to_owned()),
                ("/dev/sda".to_owned(), "newer".to_owned()),
            ])
        )
    );
}

#[test]
fn ending_before_any_frame_is_a_refusal() {
    assert_eq!(
        evolve(
            Link::Starting,
            Event::Ended("a password is required".to_owned())
        ),
        Link::Refused("a password is required".to_owned())
    );
}

#[test]
fn ending_after_frames_is_a_lost_stream() {
    let link = evolve(Link::Starting, Event::Read(Frame::default()));
    assert_eq!(
        evolve(link, Event::Ended("killed".to_owned())),
        Link::Lost("killed".to_owned())
    );
}

#[test]
fn an_ended_link_stays_ended() {
    let refused = Link::Refused("no".to_owned());
    assert_eq!(
        evolve(refused, Event::Read(Frame::default())),
        Link::Refused("no".to_owned())
    );
    let lost = Link::Lost("gone".to_owned());
    assert_eq!(
        evolve(lost, Event::Ended("again".to_owned())),
        Link::Lost("gone".to_owned())
    );
}

#[test]
fn frames_advance_only_at_their_end() {
    let link = Mutex::new(Link::Starting);
    frames(
        Cursor::new("msr 0 0x10 0x1\nend\nmsr 0 0x10 0x2\nnoise\nend\nmsr 0 0x10 0x3\n"),
        &link,
    );
    assert_eq!(
        link.into_inner().unwrap(),
        streaming(
            Some(Registers::from([((0, 0x10), 1)])),
            Registers::from([((0, 0x10), 2)]),
            Reports::new()
        )
    );
}

#[test]
fn a_helper_that_streams_and_exits_reports_its_last_error_line() {
    let link = Mutex::new(Link::Starting);
    let why = run(
        &owned(&[
            "sh",
            "-c",
            "printf 'msr 0 0x10 0x1\\nend\\n'; printf 'first\\nlast\\n\\n' >&2",
        ]),
        &link,
    );
    assert_eq!(why, "last");
    assert_eq!(
        link.into_inner().unwrap(),
        streaming(None, Registers::from([((0, 0x10), 1)]), Reports::new())
    );
}

#[test]
fn a_silent_failure_reports_its_exit_status() {
    let link = Mutex::new(Link::Starting);
    assert_eq!(run(&owned(&["false"]), &link), "exit status: 1");
    assert_eq!(link.into_inner().unwrap(), Link::Starting);
}

#[test]
fn a_missing_program_says_so() {
    let link = Mutex::new(Link::Starting);
    let why = run(&owned(&["/nonexistent/sysglance-helper"]), &link);
    assert!(why.starts_with("/nonexistent/sysglance-helper: "), "{why}");
}

#[test]
fn no_command_runs_nothing() {
    assert_eq!(run(&[], &Mutex::new(Link::Starting)), "no helper command");
}

#[test]
fn a_starting_helper_is_pending_and_names_its_command() {
    let found = collected(Link::Starting, true);
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].readings[0].value,
        Value::Unavailable(Unavailable::Pending)
    );
    assert_eq!(found[0].readings[0].origin, "sudo -n helper");
}

#[test]
fn a_refused_helper_needs_privilege_and_shows_why() {
    let found = collected(
        Link::Refused("sudo: a password is required".to_owned()),
        true,
    );
    assert_eq!(
        found[0].readings[0].value,
        Value::Unavailable(Unavailable::NeedsPrivilege(GRANT))
    );
    assert_eq!(found[0].readings[0].origin, "sudo: a password is required");
}

#[test]
fn a_lost_helper_is_unreadable() {
    let found = collected(Link::Lost("killed".to_owned()), true);
    assert_eq!(
        found[0].readings[0].value,
        Value::Unavailable(Unavailable::Unreadable("helper exited: killed".to_owned()))
    );
}

#[test]
fn a_streaming_helper_shows_registers_and_one_card_per_drive() {
    let link = streaming(
        None,
        Registers::new(),
        Reports::from([
            ("/dev/nvme0".to_owned(), "{}".to_owned()),
            ("/dev/sda".to_owned(), "{}".to_owned()),
        ]),
    );
    let keys: Vec<String> = collected(link, true).into_iter().map(|d| d.key).collect();
    assert_eq!(
        keys,
        vec!["msr:package", "smart:/dev/nvme0", "smart:/dev/sda"]
    );
}

#[test]
fn registers_of_an_undecodable_cpu_are_not_supported() {
    let link = streaming(None, Registers::from([((0, 0x10), 1)]), Reports::new());
    let found = collected(link, false);
    assert_eq!(
        found[0].readings[0].value,
        Value::Unavailable(Unavailable::NotSupported)
    );
}
