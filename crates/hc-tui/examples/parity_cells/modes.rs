//! Outer ModifyOtherKeys state, which the pinned native API cannot query.
//! Ignore control strings and retain incomplete CSI across stream chunks.
#[derive(Default)]
pub struct Modes {
    state: u8,
    csi: Vec<u8>,
    pub modify_other_keys: u8,
}

impl Modes {
    pub fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            match (self.state, byte) {
                (_, 0x18 | 0x1a) => self.state = 0,
                (3, 0x07 | 0x9c) | (4, b'\\') => self.state = 0,
                (3, 0x1b) => self.state = 4,
                (3, _) => {}
                (4, _) => self.state = 3,
                (_, 0x1b) => self.state = 1,
                (1, b'[') => {
                    self.state = 2;
                    self.csi.clear();
                }
                (1, b']' | b'P' | b'X' | b'^' | b'_') => self.state = 3,
                (1, b'c') => {
                    self.modify_other_keys = 0;
                    self.state = 0;
                }
                (1, _) => self.state = 0,
                (2, 0x40..=0x7e) => {
                    if byte == b'm'
                        && let Some(parameters) = self.csi.strip_prefix(b">")
                        && let Ok(parameters) = std::str::from_utf8(parameters)
                    {
                        let mut fields = parameters.split(';');
                        let kind = fields.next().and_then(|s| s.parse::<u16>().ok());
                        let level = fields.next().unwrap_or("0");
                        let level = if level.is_empty() {
                            Some(0)
                        } else {
                            level.parse::<u8>().ok()
                        };
                        if kind == Some(4)
                            && fields.next().is_none()
                            && let Some(level @ 0..=2) = level
                        {
                            self.modify_other_keys = level;
                        }
                    }
                    if byte == b'p' && self.csi == b"!" {
                        self.modify_other_keys = 0; // DECSTR resets this native flag too.
                    }
                    self.state = 0;
                }
                (2, _) if self.csi.len() < 64 => self.csi.push(byte),
                (2, _) => self.state = 0,
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn split_modes_and_string_payloads() {
        let mut modes = Modes::default();
        for byte in b"\x1b[>4;2m" {
            modes.feed(&[*byte]);
        }
        assert_eq!(modes.modify_other_keys, 2);
        modes.feed(b"\x1b]2;title \x1b[>4;0m\x07");
        assert_eq!(modes.modify_other_keys, 2);
        modes.feed(b"\x1bPfake \x1b[>4;0m\x1b\\");
        assert_eq!(modes.modify_other_keys, 2);
        modes.feed(b"\x1b[>4;1m");
        assert_eq!(modes.modify_other_keys, 1);
        modes.feed(b"\x1b[>4m");
        assert_eq!(modes.modify_other_keys, 0);
        modes.feed(b"\x1b[>04;02m");
        assert_eq!(modes.modify_other_keys, 2);
        modes.feed(b"\x1b[!p");
        assert_eq!(modes.modify_other_keys, 0);
    }
}
