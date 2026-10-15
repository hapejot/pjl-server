//! Bildschirm-Aufbau: geschuetzte Texte und Eingabefelder, uebersetzt in
//! einen 3270-Datenstrom (srv-3270 Encoder).

use srv_3270::{Attribute, Color, Command, DataStream, FieldAttribute, Order, Wcc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldId {
    Select(usize),
    Command,
    Named(&'static str),
}

#[derive(Debug, Clone)]
pub struct InputField {
    pub id: FieldId,
    /// Lineare Position des Feld-Attributbytes (row * cols + col).
    pub pos: usize,
    pub row: u16,
    pub col: u16,
    pub len: u16,
}

pub struct Screen {
    pub cols: u16,
    orders: Vec<Order>,
    pub inputs: Vec<InputField>,
    cursor: Option<(u16, u16)>,
}

/// Nur ASCII ausgeben. Umlaute werden umschrieben, Rest wird ersetzt.
pub fn ascii(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'ä' => out.push_str("ae"),
            'ö' => out.push_str("oe"),
            'ü' => out.push_str("ue"),
            'Ä' => out.push_str("Ae"),
            'Ö' => out.push_str("Oe"),
            'Ü' => out.push_str("Ue"),
            'ß' => out.push_str("ss"),
            c if c.is_ascii_graphic() || c == ' ' => out.push(c),
            c if c.is_control() || c.is_whitespace() => out.push(' '),
            _ => out.push('?'),
        }
    }
    out
}

pub fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

fn pad(s: &str, len: usize) -> String {
    let mut t = clip(s, len);
    while t.chars().count() < len {
        t.push(' ');
    }
    t
}

impl Screen {
    pub fn new(_rows: u16, cols: u16) -> Self {
        Self { cols, orders: Vec::new(), inputs: Vec::new(), cursor: None }
    }

    /// Geschuetztes Textfeld ab (row, col). Laenge wird auf die Zeile gekappt.
    pub fn text(&mut self, row: u16, col: u16, s: &str, fa: FieldAttribute, color: Option<Color>) {
        let max = self.cols.saturating_sub(col + 1) as usize;
        let s = clip(&ascii(s), max);
        self.orders.push(Order::SetBufferAddress { row, col });
        self.orders.push(Order::StartField(fa));
        if let Some(c) = color {
            self.orders.push(Order::SetAttribute(Attribute::Foreground(c)));
            self.orders.push(Order::Text(s));
            self.orders.push(Order::SetAttribute(Attribute::Reset));
        } else {
            self.orders.push(Order::Text(s));
        }
    }

    /// Ungeschuetztes Eingabefeld der Laenge `len`, Datenbeginn bei col+1.
    pub fn input(&mut self, id: FieldId, row: u16, col: u16, len: u16, initial: &str) {
        self.input_with(id, row, col, len, initial, FieldAttribute::unprotected_normal());
    }

    /// Wie `input`, aber mit eigenem Feldattribut (z.B. numerisch).
    pub fn input_with(
        &mut self,
        id: FieldId,
        row: u16,
        col: u16,
        len: u16,
        initial: &str,
        fa: FieldAttribute,
    ) {
        let data = pad(&ascii(initial), len as usize);
        self.orders.push(Order::SetBufferAddress { row, col });
        self.orders.push(Order::StartField(fa));
        self.orders.push(Order::Text(data));
        self.inputs.push(InputField {
            id,
            pos: row as usize * self.cols as usize + col as usize,
            row,
            col,
            len,
        });
    }

    /// Setzt den Cursor auf den Datenbeginn eines vorhandenen Eingabefeldes.
    pub fn focus(&mut self, id: FieldId) {
        if let Some(f) = self.inputs.iter().find(|f| f.id == id) {
            self.cursor = Some((f.row, f.col + 1));
        }
    }

    pub fn to_stream(&self) -> DataStream {
        let mut orders = self.orders.clone();
        if let Some((row, col)) = self.cursor {
            orders.push(Order::SetBufferAddress { row, col });
            orders.push(Order::InsertCursor);
        }
        DataStream {
            command: Command::EraseWriteAlternate,
            wcc: Wcc::reset_restore_mdt(),
            width: self.cols,
            orders,
        }
    }
}
