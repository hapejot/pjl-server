//! Sitzung pro 3270-Verbindung: Navigationsstack aus Views.

use serde_json::Value;
use tracing::info;

use crate::context::Context;
use crate::odata::Odata;
use crate::screen::{FieldId, InputField, Screen};
use crate::tn3270::{Aid, Inbound, Terminal};
use crate::views::{self, Action, App, View};
use anyhow::Result;
use tokio::net::TcpStream;

#[derive(PartialEq, Eq)]
pub enum Flow {
    Continue,
    Exit,
}

pub struct Session {
    app: App,
    stack: Vec<Box<dyn View>>,
}

impl Session {
    pub fn new(odata: Odata, rows: u16, cols: u16) -> Self {
        Self {
            app: App {
                odata,
                ctx: Context::default(),
                msg: String::new(),
                rows,
                cols,
            },
            stack: Vec::new(),
        }
    }

    /// Öffnet das Startview (Liste).
    pub async fn start(&mut self) -> Result<(), String> {
        let mut root = views::create("list").ok_or("Startview fehlt")?;
        root.enter(&mut self.app, Value::Null).await?;
        self.stack.push(root);
        Ok(())
    }

    pub fn render(&self) -> Screen {
        match self.stack.last() {
            Some(view) => view.render(&self.app),
            None => Screen::new(self.app.rows, self.app.cols),
        }
    }

    pub async fn handle(&mut self, aid: Aid, values: Vec<(FieldId, String)>) -> Flow {
        self.app.msg.clear();
        let Some(top) = self.stack.last_mut() else {
            return Flow::Exit;
        };
        match top.handle(&mut self.app, aid, &values).await {
            Action::Stay => Flow::Continue,
            Action::Exit => Flow::Exit,
            Action::Back => self.back(),
            Action::Open { view, params } => self.open(view, params).await,
        }
    }

    /// Im Startview beendet Zurück die Sitzung.
    fn back(&mut self) -> Flow {
        if self.stack.len() > 1 {
            self.stack.pop();
            Flow::Continue
        } else {
            Flow::Exit
        }
    }

    /// Der neue View wird nur auf den Stack gelegt, wenn sein Aufbau gelingt.
    async fn open(&mut self, name: &str, params: Value) -> Flow {
        let Some(mut view) = views::create(name) else {
            self.app.msg = format!("Unbekannter Screen: {name}");
            return Flow::Continue;
        };
        match view.enter(&mut self.app, params).await {
            Ok(()) => self.stack.push(view),
            Err(e) => self.app.msg = e,
        }
        Flow::Continue
    }
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

pub async fn run(stream: TcpStream, odata: Odata) -> Result<()> {
    let mut term = Terminal::negotiate(stream).await?;
    info!("ausgehandelte Terminalgröße: {}x{}", term.cols, term.rows);
    let mut session = Session::new(odata, term.rows, term.cols);
    session.start().await.map_err(anyhow::Error::msg)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn session() -> Session {
        let odata = Odata::new("http://localhost:1/Service".into(), "u".into()).unwrap();
        Session::new(odata, 24, 80)
    }

    #[tokio::test]
    async fn pf3_on_list_ends_session() {
        let mut s = session();
        s.start().await.unwrap();
        assert!(matches!(s.handle(Aid::Pf(3), Vec::new()).await, Flow::Exit));
    }

    #[tokio::test]
    async fn failed_detail_load_keeps_list_on_stack() {
        let mut s = session();
        s.start().await.unwrap();
        s.app.ctx.set("/list/items", json!([{ "ID": "a" }]));
        let flow = s
            .handle(Aid::Enter, vec![(FieldId::Select(0), "x".into())])
            .await;
        assert!(matches!(flow, Flow::Continue));
        assert_eq!(s.stack.len(), 1);
        assert!(s.app.msg.starts_with("Fehler beim Laden des Details"));
    }

    #[tokio::test]
    async fn pf3_in_detail_returns_to_list() {
        let mut s = session();
        s.start().await.unwrap();
        s.app
            .ctx
            .set("/detail/video", json!({ "ID": "a", "title": "Film" }));
        s.app.ctx.set("/detail/idx", json!(0));
        s.stack.push(views::create("detail").unwrap());
        assert_eq!(s.stack.len(), 2);

        let flow = s.handle(Aid::Pf(3), Vec::new()).await;
        assert!(matches!(flow, Flow::Continue));
        assert_eq!(s.stack.len(), 1);
    }

    #[tokio::test]
    async fn unknown_screen_is_reported_not_pushed() {
        let mut s = session();
        s.start().await.unwrap();
        let flow = s.open("gibtsnicht", Value::Null).await;
        assert!(matches!(flow, Flow::Continue));
        assert_eq!(s.stack.len(), 1);
        assert_eq!(s.app.msg, "Unbekannter Screen: gibtsnicht");
    }
}
