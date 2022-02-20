use std::env::args;
use std::fs;
use std::path::PathBuf;
use std::io::{Read, stdin};
use atty::Stream;
use copypasta::{ClipboardContext, ClipboardProvider};

fn write<T: AsRef<str>>(content: T) {
    let mut ctx: ClipboardContext = ClipboardContext::new().unwrap();
    ctx.set_contents(content.as_ref().to_owned()).unwrap();
}

fn print() {
    let mut ctx: ClipboardContext = ClipboardContext::new().unwrap();
    println!("{}", ctx.get_contents().unwrap());
}

fn main() {
    // Handle `cp ...` stdin cases
    // echo 'foo' | cp
    if atty::isnt(Stream::Stdin) {
        let mut buffer = String::new();
        stdin().read_to_string(&mut buffer).unwrap();
        write(buffer.trim());
        return;
    }

    // Handle `cp ...` stdout cases
    match args().len() {
        // Case of `cp`
        1 => { return print() },
        // Case of `cp my-file`
        2 => match args().nth(1) {
            Some(path) => {
                let abs_path = PathBuf::from(path).canonicalize().unwrap();
                let res = fs::read_to_string(abs_path).expect("file does not exist");
                return write(res);
            }
            None => {}
        },
        _ => panic!("unexpected number of args")
    }
}
