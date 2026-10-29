use super::{FILL, Rgb, raster};

const WHITE: Rgb = Rgb {
    red: 255,
    green: 255,
    blue: 255,
};

fn alpha(image: &[u8], width: usize, x: usize, y: usize) -> u8 {
    image[(y * width + x) * 4 + 3]
}

fn column(image: &[u8], width: usize, height: usize, x: usize) -> Vec<u8> {
    (0..height).map(|y| alpha(image, width, x, y)).collect()
}

#[test]
fn no_bars_paint_nothing() {
    assert!(raster(&[], 4, 4, 4, WHITE).iter().all(|&byte| byte == 0));
}

#[test]
fn a_full_bar_fills_its_column_under_a_solid_top() {
    let image = raster(&[1.0, 1.0], 2, 2, 3, WHITE);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the test fill alpha"
    )]
    let fill = (255.0 * FILL).round() as u8;
    assert_eq!(column(&image, 2, 3, 0), [255, fill, fill]);
}

#[test]
fn an_empty_bar_paints_only_the_baseline() {
    let image = raster(&[0.0, 0.0], 2, 2, 3, WHITE);
    assert_eq!(column(&image, 2, 3, 1), [0, 0, 255]);
}

#[test]
fn a_short_history_sits_at_the_right() {
    let image = raster(&[1.0, 1.0], 5, 5, 2, WHITE);
    let tops: Vec<u8> = (0..5).map(|x| alpha(&image, 5, x, 0)).collect();
    assert_eq!(tops, [0, 0, 0, 255, 255]);
}

#[test]
fn values_between_bars_are_interpolated() {
    let image = raster(&[0.0, 1.0], 2, 3, 3, WHITE);
    assert_eq!(column(&image, 3, 3, 1)[1], 255);
}

#[test]
fn interpolation_is_between_the_neighbouring_bars() {
    let image = raster(&[0.0, 0.5, 1.0], 3, 5, 5, WHITE);
    assert_eq!(column(&image, 5, 5, 3)[1], 255);
}

#[test]
fn a_one_pixel_wide_sparkline_paints_nothing() {
    assert!(
        raster(&[1.0, 1.0], 2, 1, 2, WHITE)
            .iter()
            .all(|&byte| byte == 0)
    );
}

#[test]
fn colour_is_stored_blue_green_red_alpha() {
    let image = raster(
        &[1.0, 1.0],
        2,
        2,
        1,
        Rgb {
            red: 10,
            green: 20,
            blue: 30,
        },
    );
    assert_eq!(&image[..4], [30, 20, 10, 255]);
}

#[test]
fn more_bars_than_slots_paint_nothing() {
    assert!(
        raster(&[1.0, 1.0, 1.0], 2, 2, 2, WHITE)
            .iter()
            .all(|&byte| byte == 0)
    );
}
