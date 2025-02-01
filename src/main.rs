
mod winloop;
mod osxloop;

fn main() {

    #[cfg(target_os = "windows")]
    winloop::message_queue();
   #[cfg(target_os = "macos") ]
   osxloop::apple_run_loop();

}
