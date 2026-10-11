//! Abbildung des Postgres-Katalogs auf ein Data Dictionary (`spec`) und die
//! Zuordnungsregeln, die der Import braucht.
//!
//! Das Modell ist die einzige Quelle der Wahrheit: `ddic.json` wird daraus
//! erzeugt, der Import baut es aus derselben Datenbank neu auf und arbeitet
//! mit denselben Regeln. `spec-schema` leitet daraus das Speicherschema ab,
//! `pjl-runtime` serviert es.

use std::collections::{BTreeMap, HashSet};

use anyhow::{bail, Context};
use data_store::Schema;
use spec::app::App;
use spec::data_dictionary::DataDictionary;
use spec::package::Package;
use spec::{EntitySpec, FieldSpec, Relationship};
use uuid::Uuid;

use crate::introspect::{PgCatalog, PgColumn, PgForeignKey, PgTable};

/// Name des Packages, unter dem alles Video-Modell liegt.
pub const PACKAGE: &str = "video";

/// Namensraum fuer deterministische UUIDs (v5). Feste Konstante: dieselbe
/// Quelltabelle und derselbe Schluessel ergeben immer dieselbe ID, deshalb
/// braucht der Import keine Mapping-Tabelle und ist wiederholbar.
pub const KEY_NAMESPACE: Uuid = Uuid::from_u128(0x6b1f_0c3e_9d2a_4f7b_8e51_2a9c_7d04_e1b3);

/// Fremdschluessel, die in der Datenbank nur implizit existieren (keine
/// Constraint). `(child_table, child_column, parent_table)`. Ziel ist immer die
/// Primaerschluessel-Spalte des Elternteils.
pub const IMPLICIT_FKS: &[(&str, &str, &str)] = &[
    ("videos", "scene_id", "scenes"),
    ("scene_files", "path_id", "scene_paths"),
    ("actor_scene_map", "actor_id", "actors"),
    ("actor_scene_map", "scene_id", "scenes"),
    ("actor_tag_map", "actor_id", "actors"),
    ("actor_tag_map", "tag_id", "actor_tags"),
    ("scene_tag_map", "scene_id", "scenes"),
    ("scene_tag_map", "tag_id", "actor_tags"),
    ("path_scene_map", "path_id", "scene_paths"),
    ("path_scene_map", "scene_id", "scenes"),
    ("emb_images", "video_id", "videos"),
    ("emb_images", "training_image_id", "training_images"),
    ("training_images", "video_id", "videos"),
];

/// Komposition erzwingen, obwohl die Datenbank keine CASCADE-Regel hat.
/// `video_jobs.agent_id` ist NOT NULL, hat aber kein ON DELETE.
pub const FORCE_COMPOSITION: &[(&str, &str)] = &[("video_jobs", "agent_id")];

/// Komposition verbieten, obwohl die Datenbank CASCADE hat.
/// `video_jobs.video_id` ist nullable (Agent-Jobs ohne Video) und darf daher
/// kein Kind einer Komposition sein.
pub const FORCE_ASSOCIATION: &[(&str, &str)] = &[("video_jobs", "video_id")];

/// Quellspalte, die als Feld `name` gefuehrt wird, wenn die Tabelle keine Spalte
/// `name` hat. Sonst wird `name` aus den Schluesseln abgeleitet.
pub const NAME_SOURCE: &[(&str, &str)] = &[("videos", "file_name"), ("scenes", "title")];

/// Wie der Primaerschluessel einer Quelltabelle in die Store-ID uebergeht.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyMode {
    /// Einzelner `uuid`-PK: die Quell-ID wird unveraendert `ID`.
    Uuid { column: String },
    /// Einzelner Nicht-UUID-PK (text, bigint, varchar): `ID` = v5(table, key),
    /// der Originalwert bleibt als `legacy_id` erhalten.
    Legacy { column: String },
    /// Zusammengesetzter oder fehlender PK: `ID` = v5 ueber alle PK-Werte
    /// bzw. ueber die ganze Zeile (leere `columns`), bei Dubletten mit Zaehler.
    Synth { columns: Vec<String> },
}

#[derive(Debug, Clone)]
pub struct AtomMap {
    /// Feldname im Store.
    pub name: String,
    /// Quellspalte. `None`: `name` wird aus den Schluesseln abgeleitet.
    pub source: Option<String>,
    pub edm: String,
    pub max_length: Option<u32>,
    pub precision: Option<u32>,
    pub scale: Option<u32>,
    /// `bytea`: Quellwert ist Hex-Text und wird als Base64 abgelegt.
    pub binary: bool,
    pub immutable: bool,
}

/// Fremdschluessel als Navigation: Quellspalte -> `{nav}_ID` -> Zieltabelle.
#[derive(Debug, Clone)]
pub struct RefMap {
    pub source: String,
    pub nav: String,
    pub store_column: String,
    pub target: String,
}

#[derive(Debug, Clone)]
pub struct TableMap {
    pub table: String,
    pub entity: String,
    pub set: String,
    pub key: KeyMode,
    pub atoms: Vec<AtomMap>,
    pub refs: Vec<RefMap>,
}

impl TableMap {
    /// Quellspalten fuer `name` ohne Namensspalte: Schluessel, sonst Fremdschluessel.
    pub fn derived_name_columns(&self) -> Vec<String> {
        match &self.key {
            KeyMode::Uuid { column } | KeyMode::Legacy { column } => vec![column.clone()],
            KeyMode::Synth { columns } if !columns.is_empty() => columns.clone(),
            KeyMode::Synth { .. } => self.refs.iter().map(|r| r.source.clone()).collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Model {
    /// Unaktiviertes Dictionary, so wie `ddic.json` es enthaelt.
    pub ddic: DataDictionary,
    /// Tabellen nach Quellname sortiert.
    pub tables: BTreeMap<String, TableMap>,
}

impl Model {
    /// Aktivierter Stand, aus dem das Speicherschema abgeleitet wird.
    pub fn activated(&self) -> DataDictionary {
        let mut ddic = self.ddic.clone();
        ddic.activate();
        ddic
    }

    pub fn schema(&self) -> anyhow::Result<Schema> {
        spec_schema::schema_from_ddic(&self.activated())
            .map_err(|e| anyhow::anyhow!("Speicherschema ungueltig: {e}"))
    }

    pub fn table_for_set(&self, set: &str) -> Option<&TableMap> {
        self.tables.values().find(|t| t.set == set)
    }
}

/// Erzeugt das Modell aus dem Katalog. Schlaegt fehl, wenn das Ergebnis kein
/// gueltiges Speicherschema ergibt.
pub fn build_model(catalog: &PgCatalog) -> anyhow::Result<Model> {
    let fks = effective_fks(catalog)?;
    let compositions = choose_compositions(catalog, &fks)?;

    let mut ddic = DataDictionary::empty();
    ddic.add_package(Package::new(PACKAGE));

    let mut tables = BTreeMap::new();
    for table in catalog.tables.values() {
        let map = table_map(catalog, table, &fks)?;
        let mut entity = EntitySpec::new(&map.entity, PACKAGE, pascal(&table.name));
        for atom in &map.atoms {
            entity = entity.field(atom_field(atom));
        }
        if !ddic.add_entity(entity) {
            bail!("Entitaet '{}' doppelt", map.entity);
        }
        tables.insert(table.name.clone(), map);
    }

    for fk in &fks {
        let composition = compositions.contains(&(fk.child.clone(), fk.child_col.clone()));
        let parent = &tables[&fk.parent];
        let child = &tables[&fk.child];
        let mut rel = Relationship::new(format!("{}_{}", fk.child, fk.child_col), PACKAGE)
            .with_one(&parent.entity, collection_nav(&child.table))
            .with_many(&child.entity, nav_name(&fk.child_col));
        if composition {
            rel = rel.owned();
        }
        if !ddic.add_relationship(rel) {
            bail!("Relationship fuer '{}.{}' doppelt", fk.child, fk.child_col);
        }
    }

    ddic.add_app(App::new(
        "videos".into(),
        "Videos".into(),
        "Video".into(),
        Some("List Videos".into()),
        None,
        Package::new(PACKAGE),
    ));

    let model = Model { ddic, tables };
    // Validierung: ein Modell, das hier nicht als Schema gilt, wird nie gestartet.
    model.schema().context("Modell wird validiert")?;
    Ok(model)
}

/// Explizite Fremdschluessel plus die fest verdrahteten impliziten.
fn effective_fks(catalog: &PgCatalog) -> anyhow::Result<Vec<PgForeignKey>> {
    let mut fks = Vec::new();
    for fk in &catalog.foreign_keys {
        let parent = catalog
            .tables
            .get(&fk.parent)
            .with_context(|| format!("Elterntabelle '{}' fehlt", fk.parent))?;
        if parent.pk.len() != 1 || parent.pk[0] != fk.parent_col {
            bail!(
                "{}.{} referenziert keine Primaerschluessel-Spalte von {}",
                fk.child,
                fk.child_col,
                fk.parent
            );
        }
        fks.push(fk.clone());
    }

    for (child, col, parent) in IMPLICIT_FKS {
        let already = fks.iter().any(|f| f.child == *child && f.child_col == *col);
        if already {
            continue;
        }
        let parent_table = catalog
            .tables
            .get(*parent)
            .with_context(|| format!("implizite FK: '{parent}' fehlt"))?;
        let child_table = catalog
            .tables
            .get(*child)
            .with_context(|| format!("implizite FK: '{child}' fehlt"))?;
        if child_table.column(col).is_none() {
            bail!("implizite FK: Spalte {child}.{col} fehlt");
        }
        if parent_table.pk.len() != 1 {
            bail!("implizite FK auf '{parent}' braucht einen einspaltigen PK");
        }
        fks.push(PgForeignKey {
            child: child.to_string(),
            child_col: col.to_string(),
            parent: parent.to_string(),
            parent_col: parent_table.pk[0].clone(),
            on_delete: 'a',
        });
    }
    Ok(fks)
}

/// Pro Kindtabelle hoechstens eine Komposition (das Schema erlaubt nur einen
/// Elternteil). Regel: CASCADE + NOT NULL, bei mehreren die erste Spalte in
/// Tabellenreihenfolge; Overrides haben Vorrang.
fn choose_compositions(
    catalog: &PgCatalog,
    fks: &[PgForeignKey],
) -> anyhow::Result<HashSet<(String, String)>> {
    let forced_assoc: HashSet<(&str, &str)> = FORCE_ASSOCIATION.iter().copied().collect();
    let mut chosen = HashSet::new();

    for table in catalog.tables.values() {
        let own: Vec<&PgForeignKey> = fks.iter().filter(|f| f.child == table.name).collect();
        let forced: Vec<&&PgForeignKey> = own
            .iter()
            .filter(|f| FORCE_COMPOSITION.contains(&(table.name.as_str(), f.child_col.as_str())))
            .collect();
        if forced.len() > 1 {
            bail!("mehrere erzwungene Kompositionen auf '{}'", table.name);
        }
        if let Some(f) = forced.first() {
            chosen.insert((f.child.clone(), f.child_col.clone()));
            continue;
        }

        let mut candidates: Vec<&&PgForeignKey> = own
            .iter()
            .filter(|f| {
                f.on_delete == 'c'
                    && !forced_assoc.contains(&(table.name.as_str(), f.child_col.as_str()))
                    && table.column(&f.child_col).is_some_and(|c| !c.nullable)
            })
            .collect();
        candidates.sort_by_key(|f| table.position(&f.child_col));
        if let Some(f) = candidates.first() {
            chosen.insert((f.child.clone(), f.child_col.clone()));
        }
    }
    Ok(chosen)
}

fn table_map(
    catalog: &PgCatalog,
    table: &PgTable,
    fks: &[PgForeignKey],
) -> anyhow::Result<TableMap> {
    let key = key_mode(table);

    let mut dropped: HashSet<String> = fks
        .iter()
        .filter(|f| f.child == table.name)
        .map(|f| f.child_col.clone())
        .collect();
    let mut atoms = Vec::new();

    match &key {
        KeyMode::Uuid { column } => {
            dropped.insert(column.clone());
        }
        KeyMode::Legacy { column } => {
            dropped.insert(column.clone());
            atoms.push(AtomMap {
                name: "legacy_id".into(),
                source: Some(column.clone()),
                edm: "Edm.String".into(),
                max_length: None,
                precision: None,
                scale: None,
                binary: false,
                immutable: true,
            });
        }
        KeyMode::Synth { .. } => {}
    }

    let name_src = name_source(table, &dropped)?;
    if name_src.is_none() {
        atoms.push(AtomMap {
            name: "name".into(),
            source: None,
            edm: "Edm.String".into(),
            max_length: None,
            precision: None,
            scale: None,
            binary: false,
            immutable: false,
        });
    }

    for col in &table.columns {
        if dropped.contains(&col.name) {
            continue;
        }
        let mut atom = atom_for(col);
        if name_src.as_deref() == Some(col.name.as_str()) {
            atom.name = "name".into();
        }
        atoms.push(atom);
    }

    for atom in &atoms {
        if is_reserved(&atom.name) {
            bail!(
                "'{}.{}' kollidiert mit einem reservierten Feld",
                table.name,
                atom.name
            );
        }
    }

    let mut refs = Vec::new();
    let mut navs: HashSet<String> = HashSet::new();
    for fk in fks.iter().filter(|f| f.child == table.name) {
        let nav = nav_name(&fk.child_col);
        if !navs.insert(nav.to_lowercase())
            || atoms.iter().any(|a| a.name.eq_ignore_ascii_case(&nav))
        {
            bail!("Navigation '{nav}' auf '{}' ist mehrdeutig", table.name);
        }
        refs.push(RefMap {
            source: fk.child_col.clone(),
            store_column: format!("{nav}_ID"),
            nav,
            target: fk.parent.clone(),
        });
    }

    // Zielschluessel muss existieren (Tabelle ohne PK kann nicht referenziert werden).
    for r in &refs {
        if !catalog.tables.contains_key(&r.target) {
            bail!("Ziel '{}' fehlt", r.target);
        }
    }

    Ok(TableMap {
        table: table.name.clone(),
        entity: entity_name(&table.name),
        set: format!("{}Set", entity_name(&table.name)),
        key,
        atoms,
        refs,
    })
}

/// Quellspalte fuer `name`. Eine vorhandene Spalte `name` hat Vorrang vor
/// [`NAME_SOURCE`]. `None`: kein Quellfeld, `name` wird abgeleitet.
fn name_source(table: &PgTable, dropped: &HashSet<String>) -> anyhow::Result<Option<String>> {
    let override_col = NAME_SOURCE
        .iter()
        .find(|(t, _)| *t == table.name)
        .map(|(_, c)| *c);
    let source = match (table.column("name").is_some(), override_col) {
        (true, Some(_)) => bail!(
            "'{}' hat eine Spalte 'name' und zusaetzlich einen NAME_SOURCE-Eintrag",
            table.name
        ),
        (true, None) => "name",
        (false, Some(col)) => col,
        (false, None) => return Ok(None),
    };
    if table.column(source).is_none() {
        bail!("'{}': Namensquelle '{source}' fehlt", table.name);
    }
    if dropped.contains(source) {
        bail!(
            "'{}': Namensquelle '{source}' ist Schluessel oder Fremdschluessel",
            table.name
        );
    }
    Ok(Some(source.to_string()))
}

fn key_mode(table: &PgTable) -> KeyMode {
    match table.pk.as_slice() {
        [single] => {
            let uuid = table.column(single).is_some_and(|c| c.udt == "uuid");
            if uuid {
                KeyMode::Uuid {
                    column: single.clone(),
                }
            } else {
                KeyMode::Legacy {
                    column: single.clone(),
                }
            }
        }
        many => KeyMode::Synth {
            columns: many.to_vec(),
        },
    }
}

fn atom_for(col: &PgColumn) -> AtomMap {
    let (edm, max_length, precision, scale) = match col.udt.as_str() {
        "text" | "varchar" | "bpchar" | "name" => ("Edm.String", col.max_length, None, None),
        "int2" | "int4" => ("Edm.Int32", None, None, None),
        "int8" => ("Edm.Int64", None, None, None),
        "float4" | "float8" => ("Edm.Double", None, None, None),
        // TODO(pg-backend): Dezimal als Text ist SQLite-Speicher; Postgres-Backend hat echte Typen.
        "numeric" => ("Edm.Decimal", None, col.precision, col.scale),
        "bool" => ("Edm.Boolean", None, None, None),
        "uuid" => ("Edm.Guid", None, None, None),
        "timestamptz" | "timestamp" => ("Edm.DateTimeOffset", None, None, None),
        "date" => ("Edm.Date", None, None, None),
        "bytea" => ("Edm.Binary", None, None, None),
        // TODO(pg-backend): jsonb, json, vector(n) und Enum-Typen werden als Text abgelegt.
        _ => ("Edm.String", None, None, None),
    };
    AtomMap {
        name: col.name.clone(),
        source: Some(col.name.clone()),
        edm: edm.into(),
        max_length,
        precision,
        scale,
        binary: col.udt == "bytea",
        immutable: false,
    }
}

fn atom_field(atom: &AtomMap) -> FieldSpec {
    FieldSpec::Atom {
        name: atom.name.clone(),
        label: atom.name.clone(),
        edm_type: atom.edm.clone(),
        package: None,
        max_length: atom.max_length,
        precision: atom.precision,
        scale: atom.scale,
        computed: false,
        immutable: atom.immutable,
        value_list: None,
        presentation: Default::default(),
    }
}

/// `video_id` -> `video`, `category_key` -> `category`, sonst unveraendert.
pub fn nav_name(column: &str) -> String {
    column
        .strip_suffix("_id")
        .or_else(|| column.strip_suffix("_key"))
        .unwrap_or(column)
        .to_string()
}

/// Sammelnavigation auf dem Elternteil: `_VideoTags`.
pub fn collection_nav(child_table: &str) -> String {
    format!("_{}", pascal(child_table))
}

pub fn entity_name(table: &str) -> String {
    pascal(&singular(table))
}

/// Singularisiert das letzte Wortsegment: `label_categories` -> `label_category`,
/// `faces` -> `face`, `emb_dinov2` bleibt.
pub fn singular(table: &str) -> String {
    let (head, last) = match table.rfind('_') {
        Some(i) => (&table[..=i], &table[i + 1..]),
        None => ("", table),
    };
    let last = if let Some(stem) = last.strip_suffix("ies") {
        format!("{stem}y")
    } else if let Some(stem) = ["ches", "shes", "xes", "sses"].iter().find_map(|suffix| {
        last.strip_suffix(suffix)
            .map(|s| s.to_string() + &suffix[..suffix.len() - 2])
    }) {
        stem
    } else if last.ends_with("ss") {
        last.to_string()
    } else if let Some(stem) = last.strip_suffix('s') {
        stem.to_string()
    } else {
        last.to_string()
    };
    format!("{head}{last}")
}

pub fn pascal(name: &str) -> String {
    name.split('_')
        .filter(|s| !s.is_empty())
        .map(|s| {
            let mut chars = s.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

fn is_reserved(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "id" | "isactiveentity"
            | "hasactiveentity"
            | "hasdraftentity"
            | "draftadministrativedata"
            | "draftmessages"
    )
}
