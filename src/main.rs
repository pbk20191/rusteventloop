use std::os::windows;
use crate::winloop::message_queue;

mod winloop;
mod osxloop;

fn main() {

    #[cfg(target_os = "windows")]
    message_queue();

    #[cfg(target_os = "macos")]
    cf_run_loop();
}

#[cfg(target_os = "windows")]
fn what1() {

}