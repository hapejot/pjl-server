//! Sitzung pro 3270-Verbindung: Zustand, Tastenbehandlung und Ansichten.

use srv_3270::{Color, FieldAttribute};
use tracing::info;

use crate::odata::{Filter, Odata, Video};
use crate::screen::{ascii, clip, FieldId, InputField, Screen};
use crate::tn3270::{Aid, Inbound, Terminal};
use anyhow::Result;
use tokio::net::TcpStream;

const LIST_TOP: u16 = 3;
const KEYS_LIST: &str =
    "PF3 Ende  PF5 Neu laden  PF7 Zurueck  PF8 Weiter  Enter=Ausfuehren/Oeffnen";
const KEYS_DETAIL: &str = "PF3 Zurueck  PF5 Speichern  PF7/PF8 Vor/Zurueck  Enter Uebernehmen";
const HELP: &str =
    "Befehle: /text  T tag  N ungesehen  X alle Filter  PF5 neu laden  S-Feld+Enter oeffnet";

#[derive(PartialEq, Eq)]
pub enum Flow {
    Continue,
    Exit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    List,
    Detail,
}

struct ListState {
    items: Vec<Video>,
    count: Option<i64>,
    page: usize,
    page_size: usize,
    filter: Filter,
}

impl ListState {
    fn skip(&self) -> usize {
        self.page * self.page_size
    }

    fn pages(&self) -> usize {
        match self.count {
            Some(c) if c > 0 => (c as usize).div_ceil(self.page_size),
            _ => 1,
        }
    }

    fn has_next(&self) -> bool {
        match self.count {
            Some(c) => self.skip() + self.page_size < c as usize,
            None => self.items.len() == self.page_size,
        }
    }
}

struct DetailState {
    idx: usize,
    video: Video,
    title: String,
    reviewed: bool,
}

impl DetailState {
    fn new(idx: usize, video: Video) -> Self {
        let title = video.title.clone().unwrap_or_default();
        let reviewed = video.is_reviewed.unwrap_or(false);
        Self {
            idx,
            video,
            title,
            reviewed,
        }
    }

    fn dirty(&self) -> bool {
        self.title != self.video.title.clone().unwrap_or_default()
            || self.reviewed != self.video.is_reviewed.unwrap_or(false)
    }
}

pub struct Session {
    odata: Odata,
    rows: u16,
    cols: u16,
    view: View,
    msg: String,
    list: ListState,
    detail: Option<DetailState>,
}

fn val<'a>(values: &'a [(FieldId, String)], id: FieldId) -> Option<&'a str> {
    values
        .iter()
        .find(|(f, _)| *f == id)
        .map(|(_, v)| v.as_str())
}

/// Ordnet die Antwortfelder anhand der Positionen den Eingabefeldern zu.
pub fn map_fields(inputs: &[InputField], fields: &[(usize, String)]) -> Vec<(FieldId, String)> {
    let mut out = Vec::new();
    for (pos, text) in fields {
        if let Some(f) = inputs
            .iter()
            .find(|f| *pos >= f.pos && *pos <= f.pos + f.len as usize)
        {
            out.push((f.id, text.trim().to_string()));
        }
    }
    out
}

fn fmt_duration(secs: Option<f64>) -> String {
    match secs {
        Some(s) => {
            let s = s.round() as u64;
            format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
        }
        None => "-".into(),
    }
}

fn fmt_size(bytes: Option<i64>) -> String {
    match bytes {
        Some(b) if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        Some(b) if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        Some(b) => format!("{b} B"),
        None => "-".into(),
    }
}

fn fmt_ts(ts: Option<&str>) -> String {
    match ts {
        Some(t) => t.replace('T', " ").chars().take(16).collect(),
        None => "-".into(),
    }
}

fn display_title(v: &Video) -> String {
    match v.title.as_deref().filter(|t| !t.is_empty()) {
        Some(t) => t.to_string(),
        None => v.name.clone().unwrap_or_default(),
    }
}

fn list_line(v: &Video) -> String {
    let res = match (v.width, v.height) {
        (Some(w), Some(h)) => format!("{w}x{h}"),
        _ => "-".into(),
    };
    let seen = match v.is_reviewed {
        Some(true) => "J",
        _ => "-",
    };
    columns(
        &clip(&display_title(v), 34),
        &fmt_duration(v.duration_seconds),
        &res,
        &clip(v.codec.as_deref().unwrap_or("-"), 6),
        &fmt_size(v.file_size),
        seen,
    )
}

fn columns(
    titel: &str,
    dauer: &str,
    aufl: &str,
    codec: &str,
    groesse: &str,
    gesehen: &str,
) -> String {
    format!("{titel:<34} {dauer:>8} {aufl:<10} {codec:<6} {groesse:>9} {gesehen}")
}

impl Session {
    pub fn new(odata: Odata, rows: u16, cols: u16) -> Self {
        let page_size = (rows.saturating_sub(LIST_TOP + 4)).max(1) as usize;
        Self {
            odata,
            rows,
            cols,
            view: View::List,
            msg: String::new(),
            list: ListState {
                items: Vec::new(),
                count: None,
                page: 0,
                page_size,
                filter: Filter::default(),
            },
            detail: None,
        }
    }

    async fn load_list(&mut self) {
        match self
            .odata
            .list(&self.list.filter, self.list.skip(), self.list.page_size)
            .await
        {
            Ok((items, count)) => {
                self.list.items = items;
                self.list.count = count;
            }
            Err(e) => self.msg = format!("Fehler beim Laden: {e}"),
        }
    }

    async fn open_detail(&mut self, idx: usize) {
        let Some(v) = self.list.items.get(idx) else {
            self.msg = "Keine Zeile unter diesem Eintrag".into();
            return;
        };
        let id = v.id.clone();
        match self.odata.get(&id).await {
            Ok(video) => {
                self.detail = Some(DetailState::new(idx, video));
                self.view = View::Detail;
                self.msg.clear();
            }
            Err(e) => self.msg = format!("Fehler beim Laden des Details: {e}"),
        }
    }

    pub async fn handle(&mut self, aid: Aid, values: Vec<(FieldId, String)>) -> Flow {
        self.msg.clear();
        match self.view {
            View::List => self.handle_list(aid, &values).await,
            View::Detail => self.handle_detail(aid, &values).await,
        }
    }

    async fn handle_list(&mut self, aid: Aid, values: &[(FieldId, String)]) -> Flow {
        match aid {
            Aid::Pf(3) | Aid::Clear => return Flow::Exit,
            Aid::Pf(5) => {
                self.load_list().await;
                self.msg = "Liste neu geladen".into();
            }
            Aid::Pf(7) => {
                if self.list.page > 0 {
                    self.list.page -= 1;
                    self.load_list().await;
                }
            }
            Aid::Pf(8) => {
                if self.list.has_next() {
                    self.list.page += 1;
                    self.load_list().await;
                }
            }
            Aid::Enter => {
                let selected = values.iter().find_map(|(id, v)| match id {
                    FieldId::Select(i) if !v.is_empty() => Some(*i),
                    _ => None,
                });
                if let Some(idx) = selected {
                    self.open_detail(idx).await;
                } else {
                    let cmd = val(values, FieldId::Command)
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    self.run_command(&cmd).await;
                }
            }
            _ => self.msg = "Taste hier nicht belegt".into(),
        }
        Flow::Continue
    }

    async fn run_command(&mut self, cmd: &str) {
        if cmd.is_empty() {
            self.load_list().await;
            self.msg = "Liste aktualisiert".into();
            return;
        }
        let (head, rest) = if let Some(r) = cmd.strip_prefix('/') {
            ("/", r.trim())
        } else {
            match cmd.find(' ') {
                Some(i) => (&cmd[..i], cmd[i + 1..].trim()),
                None => (cmd, ""),
            }
        };
        let arg = if rest.is_empty() {
            None
        } else {
            Some(rest.to_string())
        };
        match head.to_uppercase().as_str() {
            "/" | "F" => {
                self.list.filter.text = arg;
            }
            "T" => {
                self.list.filter.tag = arg;
            }
            "N" => {
                self.list.filter.only_unreviewed = true;
            }
            "X" => {
                self.list.filter = Filter::default();
            }
            "?" | "H" => {
                self.msg = HELP.into();
                return;
            }
            _ => {
                self.msg = format!("Unbekannter Befehl: {cmd}");
                return;
            }
        }
        self.list.page = 0;
        self.load_list().await;
        if self.msg.is_empty() {
            self.msg = format!(
                "{} Treffer",
                self.list.count.unwrap_or(self.list.items.len() as i64)
            );
        }
    }

    async fn handle_detail(&mut self, aid: Aid, values: &[(FieldId, String)]) -> Flow {
        match aid {
            Aid::Pf(3) | Aid::Clear => {
                self.view = View::List;
                self.detail = None;
            }
            Aid::Pf(7) => self.step_detail(-1).await,
            Aid::Pf(8) => self.step_detail(1).await,
            Aid::Pf(5) => self.save().await,
            Aid::Enter => self.apply_inputs(values),
            _ => self.msg = "Taste hier nicht belegt".into(),
        }
        Flow::Continue
    }

    async fn step_detail(&mut self, delta: isize) {
        let Some(cur) = self.detail.as_ref().map(|d| d.idx) else {
            return;
        };
        let next = cur as isize + delta;
        if next < 0 || next as usize >= self.list.items.len() {
            self.msg = "Kein weiterer Eintrag".into();
            return;
        }
        self.open_detail(next as usize).await;
    }

    fn apply_inputs(&mut self, values: &[(FieldId, String)]) {
        let Some(d) = self.detail.as_mut() else {
            return;
        };
        if let Some(t) = val(values, FieldId::Title) {
            d.title = clip(t, 512);
        }
        if let Some(r) = val(values, FieldId::Reviewed) {
            match r.to_uppercase().as_str() {
                "J" | "Y" | "1" => d.reviewed = true,
                "N" | "0" => d.reviewed = false,
                _ => {
                    self.msg = "Gesehen: J oder N eingeben".into();
                    return;
                }
            }
        }
        self.msg = if d.dirty() {
            "Uebernommen - PF5 speichert".into()
        } else {
            "Keine Aenderungen".into()
        };
    }

    async fn save(&mut self) {
        let Some(d) = self.detail.as_ref() else {
            return;
        };
        if !d.dirty() {
            self.msg = "Keine Aenderungen".into();
            return;
        }
        let (id, title, reviewed, idx) = (d.video.id.clone(), d.title.clone(), d.reviewed, d.idx);
        if let Err(e) = self.odata.patch(&id, &title, reviewed).await {
            self.msg = format!("Speichern fehlgeschlagen: {e}");
            return;
        }
        // Neu laden, damit Detail und Liste den gespeicherten Stand zeigen.
        match self.odata.get(&id).await {
            Ok(video) => {
                if let Some(item) = self.list.items.get_mut(idx) {
                    *item = video.clone();
                }
                self.detail = Some(DetailState::new(idx, video));
                self.msg = "Gespeichert".into();
            }
            Err(e) => self.msg = format!("Gespeichert, aber Neuladen fehlgeschlagen: {e}"),
        }
    }

    pub fn render(&self) -> Screen {
        match (self.view, self.detail.as_ref()) {
            (View::Detail, Some(d)) => self.render_detail(d),
            _ => self.render_list(),
        }
    }

    fn render_list(&self) -> Screen {
        let (rows, cols) = (self.rows, self.cols);
        let mut s = Screen::new(rows, cols);
        let count = self
            .list
            .count
            .map(|c| c.to_string())
            .unwrap_or_else(|| "?".into());
        s.text(
            0,
            0,
            &format!(
                "VIDEO-STORE  Videoliste   Seite {}/{}   Treffer {}",
                self.list.page + 1,
                self.list.pages(),
                count
            ),
            FieldAttribute::protected_intensified(),
            Some(Color::Turquoise),
        );
        s.text(
            1,
            0,
            &format!("Filter: {}", self.list.filter.describe()),
            FieldAttribute::normal(),
            None,
        );
        s.text(
            2,
            2,
            &columns("Titel", "Dauer", "Aufloes.", "Codec", "Groesse", "G"),
            FieldAttribute::protected_intensified(),
            None,
        );
        for i in 0..self.list.page_size {
            let row = LIST_TOP + i as u16;
            if let Some(v) = self.list.items.get(i) {
                s.input(FieldId::Select(i), row, 0, 1, "");
                s.text(row, 2, &list_line(v), FieldAttribute::normal(), None);
            }
        }
        let r_cmd = rows - 3;
        s.text(r_cmd, 0, "Befehl ==>", FieldAttribute::normal(), None);
        s.input(FieldId::Command, r_cmd, 11, cols - 12, "");
        s.focus(FieldId::Command);
        s.text(
            rows - 2,
            0,
            &self.msg,
            FieldAttribute::protected_intensified(),
            None,
        );
        s.text(rows - 1, 0, KEYS_LIST, FieldAttribute::normal(), None);
        s
    }

    fn render_detail(&self, d: &DetailState) -> Screen {
        let (rows, cols) = (self.rows, self.cols);
        let mut s = Screen::new(rows, cols);
        let v = &d.video;
        let pos = format!("{}/{}", d.idx + 1, self.list.items.len());
        s.text(
            0,
            0,
            &format!("VIDEO-STORE  Detail {pos}"),
            FieldAttribute::protected_intensified(),
            Some(Color::Turquoise),
        );
        let marker = if d.dirty() { "  *geaendert*" } else { "" };
        s.text(
            1,
            0,
            &format!("{}{marker}", v.name.clone().unwrap_or_default()),
            FieldAttribute::normal(),
            None,
        );

        let fa = FieldAttribute::normal();
        s.text(3, 2, "Titel", fa, None);
        s.input(FieldId::Title, 3, 20, 56, &d.title);
        s.text(4, 2, "Gesehen (J/N)", fa, None);
        s.input(
            FieldId::Reviewed,
            4,
            20,
            1,
            if d.reviewed { "J" } else { "N" },
        );

        let res = match (v.width, v.height) {
            (Some(w), Some(h)) => format!("{w}x{h}"),
            _ => "-".into(),
        };
        let bitrate = v
            .bitrate
            .map(|b| format!("{} kbit/s", b / 1000))
            .unwrap_or_else(|| "-".into());
        let rows_kv = [
            ("Dateipfad", v.file_path.clone().unwrap_or_default()),
            ("Dauer", fmt_duration(v.duration_seconds)),
            ("Aufloesung", res),
            ("Codec", v.codec.clone().unwrap_or_else(|| "-".into())),
            ("Bitrate", bitrate),
            ("Groesse", fmt_size(v.file_size)),
            (
                "Vorschaubilder",
                v.thumbnail_count
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "-".into()),
            ),
            ("Erfasst", fmt_ts(v.created_at.as_deref())),
            ("Aktualisiert", fmt_ts(v.updated_at.as_deref())),
            ("Zuletzt gesehen", fmt_ts(v.last_seen.as_deref())),
        ];
        for (i, (label, value)) in rows_kv.iter().enumerate() {
            let row = 6 + i as u16;
            s.text(row, 2, label, fa, None);
            s.text(row, 20, value, fa, None);
        }

        info!(?v);

        let tags: Vec<String> = v
            .tags
            .clone()
            .into_iter()
            .filter_map(|t| t.tag.and_then(|t| t.name))
            .collect();
        let tag_text = if tags.is_empty() {
            "-".to_string()
        } else {
            tags.join(", ")
        };
        let mut row = 17u16;
        s.text(row, 2, "Tags", fa, None);
        let text = ascii(&tag_text);
        let chars: Vec<char> = text.chars().collect();
        for chunk in chars.chunks(56) {
            if row >= rows - 5 {
                break;
            }
            s.text(row, 20, &chunk.iter().collect::<String>(), fa, None);
            row += 1;
        }

        s.text(
            rows - 2,
            0,
            &self.msg,
            FieldAttribute::protected_intensified(),
            None,
        );
        s.text(rows - 1, 0, KEYS_DETAIL, FieldAttribute::normal(), None);
        s.focus(FieldId::Title);
        s
    }
}

pub async fn run(stream: TcpStream, odata: Odata) -> Result<()> {
    let mut term = Terminal::negotiate(stream).await?;
    let mut session = Session::new(odata, term.rows, term.cols);
    session.load_list().await;
    loop {
        let screen = session.render();
        term.write_record(&screen.to_stream().to_bytes()).await?;
        let Some(rec) = term.read_record().await? else {
            break;
        };
        let Inbound { aid, fields } = crate::tn3270::parse_inbound(&rec);
        let values = map_fields(&screen.inputs, &fields);
        if session.handle(aid, values).await == Flow::Exit {
            break;
        }
    }
    Ok(())
}
