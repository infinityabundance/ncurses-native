//! Run a fixed window-geometry scenario and print each reported window's
//! geometry, for the NCURSES.WINDOW.GEOMETRY court.
//! Usage: `cargo run --quiet --example wgeom_eval -- <scenario 0..4>`
//! Output: one line per window `begy,begx,maxy,maxx,cury,curx,pary,parx`.

use ncurses_native::Window;

fn line(w: &Window) -> String {
    let (by, bx) = w.getbegyx();
    let (my, mx) = w.getmaxyx();
    let (cy, cx) = w.getyx();
    let (py, px) = w.getparyx();
    format!("{by},{bx},{my},{mx},{cy},{cx},{py},{px},{}", i32::from(w.is_pad()))
}

fn main() {
    let s: i32 = std::env::args().nth(1).expect("scenario").parse().unwrap();
    let mut out = Vec::new();
    match s {
        0 => {
            let mut w = Window::newwin(5, 20, 3, 7);
            w.move_to(2, 4);
            out.push(line(&w));
        }
        1 => {
            let mut w = Window::newwin(5, 20, 3, 7);
            w.mvwin(1, 2);
            out.push(line(&w));
        }
        2 => {
            let mut w = Window::newwin(5, 20, 3, 7);
            w.mvwin(1, 2);
            w.wresize(6, 10);
            w.move_to(2, 4);
            out.push(line(&w));
        }
        3 => {
            let w = Window::newwin(5, 20, 1, 2);
            let c = w.derwin(2, 5, 1, 1);
            out.push(line(&w));
            out.push(line(&c));
        }
        4 => {
            let w = Window::newwin(5, 20, 1, 2);
            let c = w.subwin(2, 5, 2, 3);
            out.push(line(&w));
            out.push(line(&c));
        }
        5 => {
            let p = Window::newpad(10, 30);
            out.push(line(&p));
            let s = p.subpad(4, 8, 1, 1);
            out.push(line(&s));
        }
        _ => {}
    }
    println!("{}", out.join("\n"));
}
