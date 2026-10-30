use std::{
    io::Cursor,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use sysglance_core::reading::{Device, Unavailable, Value};

use super::{
    Drive, Event, Frame, GRANT, Health, Helper, Line, Link, Registers, Reports, SILENCE, Watched,
    evolve, frames, parse, run,
};
use crate::sources::Source;

fn owned(command: &[&str]) -> Vec<String> {
    command.iter().map(|part| (*part).to_owned()).collect()
}

fn drive(path: &str, kind: &str) -> Drive {
    Drive {
        path: path.to_owned(),
        kind: kind.to_owned(),
    }
}

fn drives(reports: &[(&str, &str)]) -> Health {
    Health::Drives(
        reports
            .iter()
            .map(|(path, json)| (drive(path, "sat"), Ok((*json).to_owned())))
            .collect(),
    )
}

fn frame(registers: &[((u32, u32), u64)], health: Option<Health>) -> Frame {
    Frame {
        registers: registers.iter().copied().collect(),
        health,
    }
}

fn streaming(previous: Option<Registers>, latest: Registers, health: Health) -> Link {
    Link::Streaming {
        previous,
        latest,
        health,
    }
}

fn watched() -> Mutex<Watched> {
    Mutex::new(Watched::new(Link::Starting))
}

fn helper(link: Link, heard: Instant, registers_decodable: bool) -> Helper {
    Helper {
        watched: Arc::new(Mutex::new(Watched { link, heard })),
        command: "sudo -n helper".to_owned(),
        registers_decodable,
        sys: PathBuf::from("/nonexistent"),
    }
}

fn collected(link: Link, registers_decodable: bool) -> Vec<Device> {
    helper(link, Instant::now(), registers_decodable).collect(Duration::from_secs(1))
}

fn status(devices: &[Device]) -> &Value {
    &devices[0].readings[0].value
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
fn a_report_line_names_path_and_type_and_keeps_its_json_whole() {
    assert_eq!(
        parse("smart /dev/bus/0 megaraid,1 {\"a\": [1, 2]}"),
        Some(Line::Report {
            drive: drive("/dev/bus/0", "megaraid,1"),
            report: Ok("{\"a\": [1, 2]}".to_owned()),
        })
    );
}

#[test]
fn an_error_line_carries_why() {
    assert_eq!(
        parse("smart-error /dev/sda sat smartctl gave no answer within 20 s"),
        Some(Line::Report {
            drive: drive("/dev/sda", "sat"),
            report: Err("smartctl gave no answer within 20 s".to_owned()),
        })
    );
}

#[test]
fn round_unavailable_and_end_lines_parse() {
    assert_eq!(parse("smart-round"), Some(Line::Round));
    assert_eq!(
        parse("smart-unavailable smartctl is not installed"),
        Some(Line::HealthUnavailable(
            "smartctl is not installed".to_owned()
        ))
    );
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
        "smart /dev/sda sat",
        "smart-round now",
        "smart-unavailable",
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
        Event::Read(frame(
            &[((0, 0x10), 1)],
            Some(drives(&[("/dev/sda", "{}")])),
        )),
    );
    assert_eq!(
        link,
        streaming(
            None,
            Registers::from([((0, 0x10), 1)]),
            drives(&[("/dev/sda", "{}")])
        )
    );
}

#[test]
fn a_first_frame_without_a_round_waits_for_health() {
    let link = evolve(Link::Starting, Event::Read(frame(&[], None)));
    assert_eq!(link, streaming(None, Registers::new(), Health::Waiting));
}

#[test]
fn a_frame_without_a_round_keeps_the_last_health() {
    let first = evolve(
        Link::Starting,
        Event::Read(frame(
            &[((0, 0x10), 1)],
            Some(drives(&[("/dev/sda", "old")])),
        )),
    );
    let second = evolve(first, Event::Read(frame(&[((0, 0x10), 2)], None)));
    assert_eq!(
        second,
        streaming(
            Some(Registers::from([((0, 0x10), 1)])),
            Registers::from([((0, 0x10), 2)]),
            drives(&[("/dev/sda", "old")])
        )
    );
}

#[test]
fn a_new_round_replaces_every_drive_so_unplugged_drives_go() {
    let first = evolve(
        Link::Starting,
        Event::Read(frame(
            &[],
            Some(drives(&[("/dev/sda", "a"), ("/dev/sdb", "b")])),
        )),
    );
    let second = evolve(
        first,
        Event::Read(frame(&[], Some(drives(&[("/dev/sda", "a2")])))),
    );
    assert_eq!(
        second,
        streaming(
            Some(Registers::new()),
            Registers::new(),
            drives(&[("/dev/sda", "a2")])
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
    let watched = watched();
    frames(
        Cursor::new("msr 0 0x10 0x1\nend\nmsr 0 0x10 0x2\nnoise\nend\nmsr 0 0x10 0x3\n"),
        &watched,
    );
    assert_eq!(
        watched.into_inner().unwrap().link,
        streaming(
            Some(Registers::from([((0, 0x10), 1)])),
            Registers::from([((0, 0x10), 2)]),
            Health::Waiting
        )
    );
}

#[test]
fn a_round_collects_reports_and_errors_until_the_frame_ends() {
    let watched = watched();
    frames(
        Cursor::new(
            "smart /dev/sdz sat {}\nsmart-round\nsmart /dev/sda sat {}\nsmart-error /dev/sdb sat hung\nend\n",
        ),
        &watched,
    );
    assert_eq!(
        watched.into_inner().unwrap().link,
        streaming(
            None,
            Registers::new(),
            Health::Drives(Reports::from([
                (drive("/dev/sda", "sat"), Ok("{}".to_owned())),
                (drive("/dev/sdb", "sat"), Err("hung".to_owned())),
            ]))
        )
    );
}

#[test]
fn a_frame_records_when_it_was_heard() {
    let watched = Mutex::new(Watched {
        link: Link::Starting,
        heard: Instant::now().checked_sub(SILENCE * 2).unwrap(),
    });
    frames(Cursor::new("end\n"), &watched);
    assert!(watched.into_inner().unwrap().heard.elapsed() < SILENCE);
}

#[test]
fn a_helper_that_streams_and_exits_reports_its_last_error_line() {
    let watched = watched();
    let why = run(
        &owned(&[
            "sh",
            "-c",
            "printf 'msr 0 0x10 0x1\\nend\\n'; printf 'first\\nlast\\n\\n' >&2",
        ]),
        &watched,
    );
    assert_eq!(why, "last");
    assert_eq!(
        watched.into_inner().unwrap().link,
        streaming(None, Registers::from([((0, 0x10), 1)]), Health::Waiting)
    );
}

#[test]
fn a_silent_failure_reports_its_exit_status() {
    let watched = watched();
    assert_eq!(run(&owned(&["false"]), &watched), "exit status: 1");
    assert_eq!(watched.into_inner().unwrap().link, Link::Starting);
}

#[test]
fn a_missing_program_says_so() {
    let why = run(&owned(&["/nonexistent/sysglance-helper"]), &watched());
    assert!(why.starts_with("/nonexistent/sysglance-helper: "), "{why}");
}

#[test]
fn no_command_runs_nothing() {
    assert_eq!(run(&[], &watched()), "no helper command");
}

#[test]
fn a_helper_that_is_not_installed_is_never_run() {
    let mut helper = Helper::spawn(Path::new("/nonexistent/sysglance-helper"), "/proc", "/sys");
    let found = helper.collect(Duration::from_secs(1));
    assert_eq!(
        status(&found),
        &Value::Unavailable(Unavailable::NeedsPrivilege(GRANT))
    );
    assert_eq!(
        found[0].readings[0].origin,
        "/nonexistent/sysglance-helper is not installed"
    );
}

#[test]
fn a_starting_helper_is_pending_and_names_its_command() {
    let found = collected(Link::Starting, true);
    assert_eq!(found.len(), 1);
    assert_eq!(status(&found), &Value::Unavailable(Unavailable::Pending));
    assert_eq!(found[0].readings[0].origin, "sudo -n helper");
}

#[test]
fn a_helper_silent_too_long_is_unreadable_not_stale() {
    let quiet = Instant::now().checked_sub(SILENCE * 2).unwrap();
    let link = streaming(None, Registers::from([((0, 0x10), 1)]), Health::Waiting);
    let found = helper(link, quiet, true).collect(Duration::from_secs(1));
    assert_eq!(found.len(), 1);
    assert_eq!(
        status(&found),
        &Value::Unavailable(Unavailable::Unreadable(
            "the helper has sent nothing for 10 s".to_owned()
        ))
    );
}

#[test]
fn a_helper_that_never_answers_is_unreadable() {
    let quiet = Instant::now().checked_sub(SILENCE * 2).unwrap();
    let found = helper(Link::Starting, quiet, true).collect(Duration::from_secs(1));
    assert!(matches!(
        status(&found),
        Value::Unavailable(Unavailable::Unreadable(_))
    ));
}

#[test]
fn a_refused_helper_needs_privilege_and_shows_why() {
    let found = collected(
        Link::Refused("sudo: a password is required".to_owned()),
        true,
    );
    assert_eq!(
        status(&found),
        &Value::Unavailable(Unavailable::NeedsPrivilege(GRANT))
    );
    assert_eq!(found[0].readings[0].origin, "sudo: a password is required");
}

#[test]
fn a_lost_helper_is_unreadable() {
    let found = collected(Link::Lost("killed".to_owned()), true);
    assert_eq!(
        status(&found),
        &Value::Unavailable(Unavailable::Unreadable("helper exited: killed".to_owned()))
    );
}

#[test]
fn a_streaming_helper_shows_registers_and_one_card_per_drive() {
    let health = Health::Drives(Reports::from([
        (drive("/dev/nvme0", "nvme"), Ok("{}".to_owned())),
        (drive("/dev/sda", "sat"), Err("hung".to_owned())),
    ]));
    let link = streaming(None, Registers::new(), health);
    let keys: Vec<String> = collected(link, true).into_iter().map(|d| d.key).collect();
    assert_eq!(
        keys,
        vec!["msr:package", "smart:/dev/nvme0 nvme", "smart:/dev/sda sat"]
    );
}

#[test]
fn health_that_is_unavailable_is_one_card_saying_why() {
    let link = streaming(
        None,
        Registers::new(),
        Health::Unavailable("smartctl is not installed".to_owned()),
    );
    let found = collected(link, true);
    let smart = found.iter().find(|d| d.key == "smart").unwrap();
    assert_eq!(
        smart.readings[0].value,
        Value::Unavailable(Unavailable::Unreadable(
            "smartctl is not installed".to_owned()
        ))
    );
}

#[test]
fn registers_of_an_undecodable_cpu_are_not_supported() {
    let link = streaming(None, Registers::from([((0, 0x10), 1)]), Health::Waiting);
    let found = collected(link, false);
    assert_eq!(
        status(&found),
        &Value::Unavailable(Unavailable::NotSupported)
    );
}
