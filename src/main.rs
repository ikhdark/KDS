mod cli;
mod digest;
mod doctor;
mod evidence;
mod gain;
mod gc;
mod hash;
mod hook;
mod init_codex;
mod logs;
mod runner;
mod storage;
mod summarize;
mod update;

fn main() {
    let code = match cli::run() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("kds: {err:#}");
            1
        }
    };
    std::process::exit(code);
}
