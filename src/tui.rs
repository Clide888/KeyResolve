//! A simple, accessible terminal menu. Editing never grabs the keyboard.
use crate::config::{self, Config, KeyPair};
use anyhow::{Context, Result, bail};
use std::{io::{self, IsTerminal, Write}, path::Path};

const NAMED: &[(&str, u16)] = &[
    ("ESC", 1), ("1", 2), ("2", 3), ("3", 4), ("4", 5), ("5", 6),
    ("6", 7), ("7", 8), ("8", 9), ("9", 10), ("0", 11),
    ("Q", 16), ("W", 17), ("E", 18), ("R", 19), ("T", 20), ("Y", 21),
    ("U", 22), ("I", 23), ("O", 24), ("P", 25), ("A", 30), ("S", 31),
    ("D", 32), ("F", 33), ("G", 34), ("H", 35), ("J", 36), ("K", 37),
    ("L", 38), ("Z", 44), ("X", 45), ("C", 46), ("V", 47), ("B", 48),
    ("N", 49), ("M", 50), ("SPACE", 57), ("LEFTCTRL", 29), ("LEFTSHIFT", 42),
    ("RIGHTSHIFT", 54), ("LEFTALT", 56), ("RIGHTCTRL", 97), ("RIGHTALT", 100),
    ("UP", 103), ("LEFT", 105), ("RIGHT", 106), ("DOWN", 108),
    ("ENTER", 28), ("TAB", 15), ("BACKSPACE", 14),
];

fn parse_key(text: &str) -> Result<u16> {
    let name = text.trim().to_ascii_uppercase();
    let name = name.strip_prefix("KEY_").unwrap_or(&name);
    if let Some((_, code)) = NAMED.iter().find(|(key, _)| *key == name) {
        return Ok(*code);
    }
    if let Some(number) = name.strip_prefix('#') {
        let code: u16 = number.parse().context("Expected a decimal Linux key code after #")?;
        if (1..=0x2ff).contains(&code) { return Ok(code); }
    }
    bail!("Unknown key '{text}'. Use a listed name or # followed by a Linux evdev key code")
}

fn name(code: u16) -> String {
    NAMED.iter().find(|(_, value)| *value == code)
        .map(|(name, _)| (*name).to_owned()).unwrap_or_else(|| format!("#{code}"))
}

fn prompt(question: &str) -> Result<String> {
    print!("{question}");
    io::stdout().flush()?;
    let mut input = String::new();
    if io::stdin().read_line(&mut input)? == 0 { bail!("Input closed; changes not saved"); }
    Ok(input.trim().to_owned())
}

pub fn run(path: &Path) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("The configuration menu requires an interactive terminal");
    }
    let mut config = Config::load(path)?.context("No saved keyboard. Run 'KeyResolve setup' first.")?;
    let mut pairs = config.pairs.clone();
    loop {
        println!("\n=== KeyResolve | Snap Tap pairs ===\nKeyboard: {}\nConfig: {}", config.keyboard.name, path.display());
        if pairs.is_empty() { println!("  (no pairs — all keys pass through)"); }
        for (i, pair) in pairs.iter().enumerate() {
            println!("  {}. {} ↔ {}", i + 1, name(pair.keys[0]), name(pair.keys[1]));
        }
        println!("\n[A] Add pair   [R] Remove pair   [D] WASD defaults   [S] Save   [Q] Quit without saving");
        match prompt("> ")?.to_ascii_lowercase().as_str() {
            "a" => {
                println!("Enter key names (letters, digits, arrows, SPACE, modifiers, ENTER, TAB, ESC), or # followed by a Linux evdev key code.");
                let first = prompt("First key: ")?;
                let second = prompt("Second key: ")?;
                let result = (|| -> Result<()> {
                    let pair = KeyPair { keys: [parse_key(&first)?, parse_key(&second)?] };
                    let mut updated = pairs.clone();
                    updated.push(pair);
                    config::validate_pairs(&updated)?;
                    pairs = updated;
                    Ok(())
                })();
                if let Err(error) = result { println!("Cannot add pair: {error:#}"); }
            }
            "r" => {
                let selection = prompt("Pair number to remove: ")?;
                match selection.parse::<usize>() {
                    Ok(n) if n > 0 && n <= pairs.len() => { pairs.remove(n - 1); }
                    _ => println!("Invalid pair number."),
                }
            }
            "d" => pairs = config::default_pairs(),
            "s" => {
                // Reload before saving so a reconnect's updated event path isn't lost.
                config = Config::load(path)?.context("Configuration was removed; changes not saved")?;
                config.pairs = pairs;
                config.save(path)?;
                println!("Saved. Restart KeyResolve (or run 'systemctl --user restart keyresolve') to apply changes.");
                return Ok(());
            }
            "q" => { println!("Changes discarded."); return Ok(()); }
            _ => println!("Choose A, R, D, S, or Q."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_and_codes() {
        assert_eq!(parse_key("w").unwrap(), 17);
        assert_eq!(parse_key("KEY_LEFT").unwrap(), 105);
        assert_eq!(parse_key("#200").unwrap(), 200);
        assert!(parse_key("#768").is_err());
        assert!(parse_key("nonsense").is_err());
    }
}
