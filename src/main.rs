use atty::Stream;
use copypasta::{ClipboardContext, ClipboardProvider};
use std::env::args;
use std::fs;
use std::io::{stdin, Read};
use std::path::PathBuf;

fn write<T: AsRef<str>>(content: T) {
    let mut ctx: ClipboardContext = ClipboardContext::new().unwrap();
    ctx.set_contents(content.as_ref().to_owned()).unwrap();
}

fn print() {
    let mut ctx: ClipboardContext = ClipboardContext::new().unwrap();
    println!("{}", ctx.get_contents().unwrap());
}

fn main() {
    // Handle `cb ...` stdin cases
    // echo 'foo' | cb
    if atty::isnt(Stream::Stdin) {
        let mut buffer = String::new();
        stdin().read_to_string(&mut buffer).unwrap();
        write(buffer.trim());
        return;
    }

    // Handle `cb ...` stdout cases
    match args().len() {
        // Case of `cb`
        1 => return print(),
        // Case of `cb my-file`
        2 => match args().nth(1) {
            Some(path) => {
                let abs_path = PathBuf::from(path).canonicalize().unwrap();
                let res = fs::read_to_string(abs_path).expect("file does not exist");
                return write(res);
            }
            None => {}
        },
        _ => panic!("unexpected number of args"),
    }
}
