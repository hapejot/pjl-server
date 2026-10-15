//! Formulare aus Feldbeschreibungen: Labels und Eingaben untereinander,
//! mit einer Pruefung der Eingabe nach Feldart.

use srv_3270::FieldAttribute;

use crate::screen::{FieldId, Screen};

pub const LABEL_COL: u16 = 2;
pub const INPUT_COL: u16 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// Beliebiger Text bis zur Feldlaenge.
    Text,
    /// Ganze Zahl mit optionalem Vorzeichen. Das Terminal laesst nur Ziffern zu.
    #[allow(dead_code)]
    Number,
    /// Einer der angegebenen Werte, Gross- und Kleinschreibung egal.
    Choice(&'static [&'static str]),
}

#[derive(Debug, Clone)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub len: u16,
    pub kind: FieldKind,
    pub value: String,
}

impl Field {
    /// Prueft eine Eingabe und liefert den Wert in kanonischer Form.
    /// Leere Zahleneingaben sind erlaubt und bedeuten "kein Wert".
    pub fn check(&self, raw: &str) -> Result<String, String> {
        let raw = raw.trim();
        match self.kind {
            FieldKind::Text => Ok(raw.to_string()),
            FieldKind::Number => {
                if raw.is_empty() || raw.parse::<i64>().is_ok() {
                    Ok(raw.to_string())
                } else {
                    Err(format!("{}: Zahl erwartet", self.label))
                }
            }
            FieldKind::Choice(options) => options
                .iter()
                .find(|o| o.eq_ignore_ascii_case(raw))
                .map(|o| o.to_string())
                .ok_or_else(|| format!("{}: erlaubt {}", self.label, options.join(", "))),
        }
    }
}

impl Screen {
    /// Legt die Felder ab Zeile `top` untereinander an und liefert die
    /// naechste freie Zeile.
    pub fn form(&mut self, top: u16, fields: &[Field]) -> u16 {
        let mut row = top;
        for f in fields {
            self.text(row, LABEL_COL, f.label, FieldAttribute::normal(), None);
            let fa = match f.kind {
                FieldKind::Number => FieldAttribute::unprotected_normal().numeric(),
                _ => FieldAttribute::unprotected_normal(),
            };
            self.input_with(FieldId::Named(f.key), row, INPUT_COL, f.len, &f.value, fa);
            row += 1;
        }
        row
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(kind: FieldKind, len: u16) -> Field {
        Field {
            key: "k",
            label: "Feld",
            len,
            kind,
            value: String::new(),
        }
    }

    #[test]
    fn text_accepts_anything_trimmed() {
        assert_eq!(
            field(FieldKind::Text, 10).check("  Film  ").unwrap(),
            "Film"
        );
    }

    #[test]
    fn number_accepts_integers_and_empty_only() {
        let f = field(FieldKind::Number, 4);
        assert_eq!(f.check("42").unwrap(), "42");
        assert_eq!(f.check("-7").unwrap(), "-7");
        assert_eq!(f.check("").unwrap(), "");
        assert!(f.check("4a").is_err());
        assert!(f.check("1.5").is_err());
    }

    #[test]
    fn choice_is_case_insensitive_and_returns_canonical_value() {
        let f = field(FieldKind::Choice(&["J", "N"]), 1);
        assert_eq!(f.check("n").unwrap(), "N");
        assert!(f.check("X").unwrap_err().contains("J, N"));
        assert!(f.check("").is_err());
    }

    #[test]
    fn form_places_labels_and_inputs_one_row_each() {
        let mut s = Screen::new(24, 80);
        let fields = [
            Field {
                key: "a",
                label: "Titel",
                len: 10,
                kind: FieldKind::Text,
                value: "Film".into(),
            },
            Field {
                key: "b",
                label: "Nummer",
                len: 3,
                kind: FieldKind::Number,
                value: "7".into(),
            },
        ];
        assert_eq!(s.form(3, &fields), 5);
        assert_eq!(s.inputs.len(), 2);
        assert_eq!(
            (
                s.inputs[0].id,
                s.inputs[0].row,
                s.inputs[0].col,
                s.inputs[0].len
            ),
            (FieldId::Named("a"), 3, INPUT_COL, 10)
        );
        assert_eq!(
            (s.inputs[1].id, s.inputs[1].row, s.inputs[1].col),
            (FieldId::Named("b"), 4, INPUT_COL)
        );
    }
}
