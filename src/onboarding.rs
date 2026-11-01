#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Username,
    Server,
    Permissions,
    Devices,
    Hotkeys,
}

impl Step {
    pub const ALL: [Self; 5] = [
        Self::Username,
        Self::Server,
        Self::Permissions,
        Self::Devices,
        Self::Hotkeys,
    ];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|step| *step == self).unwrap()
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Username => "Your username",
            Self::Server => "Your server",
            Self::Permissions => "Permissions",
            Self::Devices => "Your audio devices",
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
            Self::Username if nickname.trim().is_empty() => Err("Enter a username to continue."),
            Self::Server if address.trim().is_empty() => {
                Err("Enter the server you want to connect to.")
            }
            _ => Ok(()),
        }
    }
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
    }
}
