//! Dump wenclose / wmouse_trafo over a fixed window, for NCURSES.MOUSE.TRAFO.
use ncurses_native::Window;
fn main() {
    let w = Window::newwin(5, 10, 2, 3);
    let mut out = String::new();
    for (y, x) in [(2, 3), (6, 12), (1, 3), (7, 7), (2, 13), (4, 7)] {
        out.push_str(&format!("enc {y},{x} {}\n", i32::from(w.wenclose(y, x))));
    }
    for (y, x, ts) in [(1, 2, true), (5, 6, true), (4, 7, false), (0, 0, false), (6, 12, false)] {
        let (ry, rx, r) = match w.mouse_trafo(y, x, ts) {
            Some((ny, nx)) => (ny, nx, 1),
            None => (y, x, 0),
        };
        out.push_str(&format!("trafo {y},{x},{} -> {ry},{rx} {r}\n", i32::from(ts)));
    }
    print!("{out}");
}
