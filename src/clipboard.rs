use std::io::Write;
use std::process::{Command, Stdio};

pub(crate) fn is_external_http_url(url: &str) -> bool {
    if url.is_empty()
        || url.trim() != url
        || url.chars().any(char::is_control)
        || url.contains('\\')
    {
        return false;
    }

    let Some((scheme, after_scheme)) = url.split_once("://") else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return false;
    }

    let authority_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let authority = &after_scheme[..authority_end];
    if authority.is_empty() {
        return false;
    }

    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };

    parsed.host().is_some()
}

pub(crate) fn open_url(url: &str) -> bool {
    for mut command in open::commands(url) {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if command.spawn().is_ok() {
            return true;
        }
    }
    false
}

pub(crate) fn copy_to_clipboard(text: &str) -> bool {
    let b64 = base64_encode(text.as_bytes());
    print!("\x1b]52;c;{b64}\x07");
    let _ = std::io::stdout().flush();

    if std::env::var("TERMUX_VERSION").is_ok() {
        return true;
    }

    let candidates: &[&[&str]] = match std::env::consts::OS {
        "macos" => &[&["pbcopy"]],
        "windows" => &[&["clip.exe"]],
        _ => &[
            &["wl-copy"],
            &["xclip", "-selection", "clipboard"],
            &["xsel", "--clipboard", "--input"],
            &["termux-clipboard-set"],
        ],
    };
    for cmd in candidates {
        if try_pipe_to_command(cmd, text).is_ok() {
            return true;
        }
    }
    false
}

fn try_pipe_to_command(cmd: &[&str], text: &str) -> std::io::Result<()> {
    let mut child = Command::new(cmd[0])
        .args(&cmd[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes())?;
    }
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("command failed"))
    }
}

fn base64_encode(input: &[u8]) -> String {
    const CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        out.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::is_external_http_url;
    use std::ffi::OsStr;

    #[test]
    fn external_http_url_policy_requires_absolute_http_host() {
        for url in [
            "http://example.test/path",
            "HTTPS://example.test/path",
            "hTtPs://user:pass@example.test:8443/path?x=1#frag",
        ] {
            assert!(is_external_http_url(url), "expected accepted URL: {url}");
        }

        for url in [
            "example.test/path",
            "//example.test/path",
            "#anchor",
            "mailto:user@example.test",
            "javascript:alert(1)",
            "file:///tmp/example",
            "https:///path-without-host",
            "http:/example.test",
            "https//example.test",
            " https://example.test/path",
            "https://example.test/path ",
            "https://\\example.test/path",
            "https://example.test/path\\next",
            "https://example.test/path\nnext",
            "https://example.test/path\u{0000}",
            "not a URL",
        ] {
            assert!(!is_external_http_url(url), "expected rejected URL: {url:?}");
        }
    }

    #[test]
    fn opener_commands_keep_hostile_url_out_of_shell_code() {
        let hostile = r##"https://example.test/?q=";&|$()`%"##;
        let target = OsStr::new(hostile);
        let commands = open::commands(hostile);
        assert!(!commands.is_empty());

        for command in commands {
            assert_ne!(command.get_program(), OsStr::new("cmd"));
            let target_arg = command.get_args().any(|arg| arg == target);
            let target_env = command
                .get_envs()
                .any(|(name, value)| name == OsStr::new("OPEN_RS_TARGET") && value == Some(target));
            assert!(
                target_arg || target_env,
                "launcher lost or embedded hostile URL: {:?}",
                command.get_program()
            );
        }
    }
}
