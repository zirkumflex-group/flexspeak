#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Username,
    Server,
    Permissions,
    Devices,
    Chat,
    Hotkeys,
}

impl Step {
    pub const ALL: [Self; 6] = [
        Self::Username,
        Self::Server,
        Self::Permissions,
        Self::Devices,
        Self::Chat,
        Self::Hotkeys,
    ];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|step| *step == self).unwrap()
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Username => "Your nickname",
            Self::Server => "Your server",
            Self::Permissions => "Permissions",
            Self::Devices => "Your audio devices",
            Self::Chat => "Your chat",
            Self::Hotkeys => "Your shortcuts",
        }
    }
    pub fn next(self) -> Option<Self> {
        Self::ALL.get(self.index() + 1).copied()
    }
    pub fn previous(self) -> Option<Self> {
        self.index().checked_sub(1).map(|index| Self::ALL[index])
    }
    pub fn validate(self, nickname: &str, address: &str) -> Result<(), &'static str> {
        match self {
            Self::Username => nickname_problem(nickname).map_or(Ok(()), Err),
            Self::Server if address.trim().is_empty() => {
                Err("Enter the server you want to connect to.")
            }
            Self::Server => normalize_address(address).map(drop),
            _ => Ok(()),
        }
    }
}

/// The fewest and most characters a TeamSpeak server accepts in a nickname.
pub const NICKNAME_LENGTH: std::ops::RangeInclusive<usize> = 3..=30;

/// What is wrong with a nickname, if anything, in a sentence for under its field.
pub fn nickname_problem(nickname: &str) -> Option<&'static str> {
    let length = nickname.trim().chars().count();
    if length == 0 {
        Some("Enter a nickname.")
    } else if length < *NICKNAME_LENGTH.start() {
        Some("A nickname needs at least 3 characters.")
    } else if length > *NICKNAME_LENGTH.end() {
        Some("A nickname can have at most 30 characters.")
    } else {
        None
    }
}

/// TeamSpeak's usual voice port, left off saved addresses.
const DEFAULT_PORT: u16 = 9987;

/// A server address as it is saved: trimmed, without a `ts3server://` link around it, the host
/// in lower case and the default port left off, so the same server is always written the same.
pub fn normalize_address(address: &str) -> Result<String, &'static str> {
    let mut rest = address.trim();
    let mut port = None;
    if rest
        .get(..12)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("ts3server://"))
    {
        rest = &rest[12..];
        if let Some((host, query)) = rest.split_once('?') {
            rest = host;
            port = query
                .split('&')
                .find_map(|pair| pair.strip_prefix("port="))
                .map(str::to_owned);
        }
        rest = rest.trim_end_matches('/');
    }
    if rest.is_empty() {
        return Err("Enter the server's address.");
    }
    if rest.chars().any(char::is_whitespace) {
        return Err("A server address can't contain spaces.");
    }
    // `[v6]:port`, `host:port`, or a bare IPv6 address, which has colons of its own.
    let (host, inline_port) = if let Some(bracketed) = rest.strip_prefix('[') {
        let (host, after) = bracketed
            .split_once(']')
            .ok_or("This address is missing its closing bracket.")?;
        (format!("[{host}]"), after.strip_prefix(':'))
    } else if rest.matches(':').count() == 1 {
        let (host, port) = rest.split_once(':').unwrap();
        (host.to_owned(), Some(port))
    } else {
        (rest.to_owned(), None)
    };
    if host.is_empty() || host == "[]" {
        return Err("Enter the server's address.");
    }
    let port = match inline_port.map(str::to_owned).or(port) {
        None => None,
        Some(port) => match port.parse::<u16>() {
            Ok(0) | Err(_) => return Err("The port must be a number from 1 to 65535."),
            Ok(port) => Some(port),
        },
    };
    let host = host.to_lowercase();
    Ok(match port {
        Some(port) if port != DEFAULT_PORT => format!("{host}:{port}"),
        _ => host,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn setup_requires_identity_and_server_and_visits_every_step_in_order() {
        let mut step = Step::Username;
        assert!(step.validate("   ", "server").is_err());
        assert!(step.previous().is_none());
        for expected in Step::ALL.iter().skip(1) {
            let next = step.next().unwrap();
            assert_eq!(next, *expected);
            assert_eq!(next.previous(), Some(step));
            step = next;
        }
        assert!(step.next().is_none());
        assert!(Step::Server.validate("Alex", "").is_err());
        assert!(Step::Server.validate("Alex", "voice.example:9987").is_ok());
        assert!(Step::Server.validate("Alex", "voice.example:port").is_err());
    }

    #[test]
    fn nicknames_are_checked_against_what_servers_accept() {
        assert!(Step::Username.validate("Al", "").is_err());
        assert!(Step::Username.validate("  Alex  ", "").is_ok());
        assert_eq!(nickname_problem(""), Some("Enter a nickname."));
        assert!(nickname_problem(&"x".repeat(30)).is_none());
        assert!(nickname_problem(&"x".repeat(31)).is_some());
        // Characters, not bytes.
        assert!(nickname_problem("Zoë").is_none());
    }

    #[test]
    fn addresses_are_saved_in_one_form() {
        let ok = |address| normalize_address(address).unwrap();
        assert_eq!(ok("  Voice.Example.ORG  "), "voice.example.org");
        assert_eq!(ok("voice.example.org:9987"), "voice.example.org");
        assert_eq!(ok("voice.example.org:10011"), "voice.example.org:10011");
        assert_eq!(ok("ts3server://Voice.Example.org"), "voice.example.org");
        assert_eq!(ok("TS3SERVER://voice.example.org/"), "voice.example.org");
        assert_eq!(
            ok("ts3server://voice.example.org?port=9988&nickname=Alex"),
            "voice.example.org:9988"
        );
        assert_eq!(
            ok("ts3server://voice.example.org?nickname=Alex&port=9987"),
            "voice.example.org"
        );
        assert_eq!(ok("10.0.0.5:9987"), "10.0.0.5");
        assert_eq!(ok("[2001:DB8::1]:9988"), "[2001:db8::1]:9988");
        assert_eq!(ok("[2001:db8::1]:9987"), "[2001:db8::1]");
        assert_eq!(ok("2001:DB8::1"), "2001:db8::1");
        assert!(normalize_address("   ").is_err());
        assert!(normalize_address("ts3server://").is_err());
        assert!(normalize_address("voice example").is_err());
        assert!(normalize_address("voice.example:0").is_err());
        assert!(normalize_address("voice.example:99999").is_err());
        assert!(normalize_address("[::1").is_err());
    }
}
