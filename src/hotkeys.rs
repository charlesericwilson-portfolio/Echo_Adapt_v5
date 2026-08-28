use std::io::{self, Write};
use std::process::Command;
use crossterm::terminal::{enable_raw_mode, disable_raw_mode};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};

pub enum InputAction {
    Submit(String),
    NewTab,
    Exit,
}

pub fn spawn_new_adapt_tab() -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let cwd = std::env::current_dir()?;

    Command::new("konsole")
        .arg("--new-tab")
        .arg("--workdir")
        .arg(cwd)
        .arg("-e")
        .arg(exe)
        .spawn()?;

    Ok(())
}

pub fn read_user_input() -> io::Result<InputAction> {
    enable_raw_mode()?;

    let mut input = String::new();

    loop {
        if let Event::Key(key) = event::read()? {
            match (key.code, key.modifiers) {
                // Ctrl+Alt+N = new Adapt process/tab
                (KeyCode::Char('n'), mods) if mods.contains(KeyModifiers::CONTROL)
                    && mods.contains(KeyModifiers::ALT)=> {
                    disable_raw_mode()?;
                    println!();
                    return Ok(InputAction::NewTab);
                }

                // Ctrl+C = exit current Adapt chat
                (KeyCode::Char('c'), mods)
                    if mods.contains(KeyModifiers::CONTROL) =>
                {
                    disable_raw_mode()?;
                    println!();
                    return Ok(InputAction::Exit);
                }

                // Enter = submit message
                (KeyCode::Enter, _) => {
                    disable_raw_mode()?;
                    println!();
                    return Ok(InputAction::Submit(input));
                }

                // Backspace
                (KeyCode::Backspace, _) => {
                    if input.pop().is_some() {
                        print!("\x08 \x08");
                        io::stdout().flush()?;
                    }
                }

                // Normal characters
                (KeyCode::Char(c), mods)
                    if !mods.contains(KeyModifiers::CONTROL)
                        && !mods.contains(KeyModifiers::ALT) =>
                {
                    input.push(c);
                    print!("{}", c);
                    io::stdout().flush()?;
                }

                _ => {}
            }
        }
    }
}
