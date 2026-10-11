//! In-Memory-Index fuer Tag-Facetten.
//!
//! Ein Video passt zur Auswahl `S`, wenn es **alle** Tags aus `S` hat (UND). Die
//! Facetten zu einer Auswahl sind die Tags, die zusammen mit `S` mindestens ein
//! Video treffen, jeweils mit der Anzahl dieser Treffer.
//!
//! Aufbau: Video und Tag werden auf kleine Ganzzahlen abgebildet. Pro Tag gibt es
//! eine sortierte Liste der Videos (invertierter Index), pro Video eine Liste der
//! Tags. Die Trefferliste zu `S` ist der Schnitt der Tag-Listen; die Facetten
//! zaehlen die Tags dieser Videos.

use std::collections::HashMap;

/// Eine verfuegbare Facette: Tag mit Trefferzahl.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facet {
    pub id: String,
    pub name: String,
    pub count: u32,
}

/// Ergebnis einer Facetten-Abfrage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacetResult {
    /// Anzahl Videos, die alle gewaehlten Tags haben.
    pub matches: u32,
    /// Verfuegbare Tags, absteigend nach Trefferzahl, dann nach Name.
    pub tags: Vec<Facet>,
}

#[derive(Debug, Default)]
pub struct TagIndex {
    tag_ids: Vec<String>,
    tag_names: Vec<String>,
    tag_lookup: HashMap<String, u32>,
    /// Je Tag: sortierte Liste der Video-Indizes.
    tag_videos: Vec<Vec<u32>>,
    /// Je Video: sortierte Liste der Tag-Indizes.
    video_tags: Vec<Vec<u32>>,
    /// Alle Tags nach Trefferzahl absteigend (ohne Auswahl), einmal beim Aufbau berechnet.
    global_ranking: Vec<u32>,
    /// Video-Index -> Video-ID.
    video_ids: Vec<String>,
    /// Video-ID -> Video-Index.
    video_lookup: HashMap<String, u32>,
    video_names: Vec<String>,
    video_durations: Vec<Option<f64>>,
    /// Rang je Video nach Name (kleiner = frueher).
    name_rank: Vec<u32>,
    /// Alle Videos nach Name sortiert, ohne Auswahl.
    name_order: Vec<u32>,
}

/// Eine Zeile der Trefferliste.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoRow {
    pub id: String,
    pub name: String,
    pub duration_seconds: Option<f64>,
}

/// Fehler bei unbekannter Tag-ID in der Auswahl.
#[derive(Debug, PartialEq, Eq)]
pub enum FacetError {
    UnknownTag(String),
}

impl TagIndex {
    /// Index aus Zuordnungen `(video_id, tag_id)` und Tag-Namen bauen. Doppelte
    /// Zuordnungen werden ignoriert. Tags ohne Namen erhalten ihre ID als Namen.
    pub fn build<I>(assignments: I, tag_names: &HashMap<String, String>) -> Self
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let mut idx = TagIndex::default();
        let mut video_lookup: HashMap<String, u32> = HashMap::new();
        let mut pairs: Vec<(u32, u32)> = Vec::new();

        for (video, tag) in assignments {
            let v = match video_lookup.get(&video) {
                Some(&v) => v,
                None => {
                    let v = video_lookup.len() as u32;
                    video_lookup.insert(video, v);
                    v
                }
            };
            let t = idx.intern_tag(&tag, tag_names);
            pairs.push((v, t));
        }

        let video_count = video_lookup.len();
        idx.video_tags = vec![Vec::new(); video_count];
        idx.tag_videos = vec![Vec::new(); idx.tag_ids.len()];
        for (v, t) in pairs {
            idx.video_tags[v as usize].push(t);
            idx.tag_videos[t as usize].push(v);
        }
        for list in idx.video_tags.iter_mut() {
            list.sort_unstable();
            list.dedup();
        }
        for list in idx.tag_videos.iter_mut() {
            list.sort_unstable();
            list.dedup();
        }
        let mut ranking: Vec<u32> = (0..idx.tag_ids.len() as u32)
            .filter(|&t| !idx.tag_videos[t as usize].is_empty())
            .collect();
        ranking.sort_by(|&a, &b| {
            idx.tag_videos[b as usize]
                .len()
                .cmp(&idx.tag_videos[a as usize].len())
                .then_with(|| idx.tag_names[a as usize].cmp(&idx.tag_names[b as usize]))
        });
        idx.global_ranking = ranking;
        idx.video_ids = {
            let mut ids = vec![String::new(); video_lookup.len()];
            for (id, &v) in &video_lookup {
                ids[v as usize] = id.clone();
            }
            ids
        };
        idx.video_names = vec![String::new(); video_lookup.len()];
        idx.video_durations = vec![None; video_lookup.len()];
        idx.video_lookup = video_lookup;
        idx.rebuild_name_order();
        idx
    }

    /// Metadaten (Name, Dauer) der Videos eintragen. Videos ohne Tag-Zuordnung
    /// bekommen einen eigenen Index, damit sie ohne Auswahl auffindbar sind.
    pub fn attach_videos<I>(&mut self, videos: I)
    where
        I: IntoIterator<Item = (String, String, Option<f64>)>,
    {
        for (id, name, duration) in videos {
            let v = match self.video_lookup.get(&id) {
                Some(&v) => v,
                None => {
                    let v = self.video_lookup.len() as u32;
                    self.video_lookup.insert(id.clone(), v);
                    self.video_ids.push(id);
                    self.video_names.push(String::new());
                    self.video_durations.push(None);
                    self.video_tags.push(Vec::new());
                    v
                }
            };
            self.video_names[v as usize] = name;
            self.video_durations[v as usize] = duration;
        }
        self.rebuild_name_order();
    }

    fn rebuild_name_order(&mut self) {
        let mut order: Vec<u32> = (0..self.video_names.len() as u32).collect();
        order.sort_by(|&a, &b| {
            self.video_names[a as usize]
                .cmp(&self.video_names[b as usize])
                .then_with(|| a.cmp(&b))
        });
        self.name_rank = vec![0; order.len()];
        for (rank, &v) in order.iter().enumerate() {
            self.name_rank[v as usize] = rank as u32;
        }
        self.name_order = order;
    }

    /// Trefferliste zur Auswahl, nach Name sortiert, seitenweise.
    /// Liefert die Gesamtzahl der Treffer und die Zeilen `skip..skip+top`.
    pub fn videos(
        &self,
        selected: &[String],
        skip: usize,
        top: usize,
    ) -> Result<(u32, Vec<VideoRow>), FacetError> {
        let sel = self.resolve(selected)?;
        let rows: Vec<u32> = if sel.is_empty() {
            self.name_order.clone()
        } else {
            let mut matched = self.intersect(&sel);
            matched.sort_unstable_by_key(|&v| self.name_rank[v as usize]);
            matched
        };
        let total = rows.len() as u32;
        let page = rows
            .into_iter()
            .skip(skip)
            .take(top)
            .map(|v| VideoRow {
                id: self.video_ids[v as usize].clone(),
                name: self.video_names[v as usize].clone(),
                duration_seconds: self.video_durations[v as usize],
            })
            .collect();
        Ok((total, page))
    }

    /// Auswahl-IDs in Tag-Indizes umsetzen: sortiert, ohne Duplikate.
    fn resolve(&self, selected: &[String]) -> Result<Vec<u32>, FacetError> {
        let mut sel: Vec<u32> = Vec::with_capacity(selected.len());
        for id in selected {
            let t = *self
                .tag_lookup
                .get(id)
                .ok_or_else(|| FacetError::UnknownTag(id.clone()))?;
            sel.push(t);
        }
        sel.sort_unstable();
        sel.dedup();
        Ok(sel)
    }

    fn intern_tag(&mut self, id: &str, names: &HashMap<String, String>) -> u32 {
        if let Some(&t) = self.tag_lookup.get(id) {
            return t;
        }
        let t = self.tag_ids.len() as u32;
        self.tag_ids.push(id.to_string());
        self.tag_names
            .push(names.get(id).cloned().unwrap_or_else(|| id.to_string()));
        self.tag_lookup.insert(id.to_string(), t);
        t
    }

    pub fn video_count(&self) -> usize {
        self.video_names.len()
    }

    pub fn tag_count(&self) -> usize {
        self.tag_ids.len()
    }

    /// Facetten zur Auswahl `selected` (Tag-IDs). `top` begrenzt die Ausgabe.
    pub fn facets(&self, selected: &[String], top: usize) -> Result<FacetResult, FacetError> {
        let mut sel: Vec<u32> = Vec::with_capacity(selected.len());
        for id in selected {
            let t = *self
                .tag_lookup
                .get(id)
                .ok_or_else(|| FacetError::UnknownTag(id.clone()))?;
            sel.push(t);
        }
        sel.sort_unstable();
        sel.dedup();

        if sel.is_empty() {
            // Ohne Auswahl ist die Rangfolge fix: nur die ersten Eintraege ausgeben.
            let tags = self
                .global_ranking
                .iter()
                .take(top)
                .map(|&t| self.facet(t, self.tag_videos[t as usize].len() as u32))
                .collect();
            return Ok(FacetResult {
                matches: self.video_names.len() as u32,
                tags,
            });
        }

        let matched = self.intersect(&sel);
        // Dichtes Zaehlfeld statt HashMap: bei vielen Tags ist das deutlich schneller.
        let mut counts = vec![0u32; self.tag_ids.len()];
        for &v in &matched {
            for &t in &self.video_tags[v as usize] {
                counts[t as usize] += 1;
            }
        }
        Ok(FacetResult {
            matches: matched.len() as u32,
            tags: self.rank(
                counts
                    .into_iter()
                    .enumerate()
                    .map(|(t, c)| (t as u32, c)),
                &sel,
                top,
            ),
        })
    }

    /// Schnitt der Video-Listen aller gewaehlten Tags (sortiert, ohne Duplikate).
    fn intersect(&self, sel: &[u32]) -> Vec<u32> {
        let mut lists: Vec<&Vec<u32>> = sel.iter().map(|&t| &self.tag_videos[t as usize]).collect();
        lists.sort_by_key(|l| l.len());
        let mut result: Vec<u32> = lists[0].clone();
        for list in &lists[1..] {
            result = intersect_sorted(&result, list);
            if result.is_empty() {
                break;
            }
        }
        result
    }

    fn rank<I>(&self, counts: I, selected: &[u32], top: usize) -> Vec<Facet>
    where
        I: Iterator<Item = (u32, u32)>,
    {
        let mut items: Vec<(u32, u32)> = counts
            .filter(|(t, c)| *c > 0 && selected.binary_search(t).is_err())
            .collect();
        items.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| self.tag_names[a.0 as usize].cmp(&self.tag_names[b.0 as usize]))
        });
        items
            .into_iter()
            .take(top)
            .map(|(t, c)| self.facet(t, c))
            .collect()
    }

    fn facet(&self, t: u32, count: u32) -> Facet {
        Facet {
            id: self.tag_ids[t as usize].clone(),
            name: self.tag_names[t as usize].clone(),
            count,
        }
    }
}

fn intersect_sorted(a: &[u32], b: &[u32]) -> Vec<u32> {
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::with_capacity(a.len().min(b.len()));
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                out.push(a[i]);
                i += 1;
                j += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    fn names(list: &[(&str, &str)]) -> HashMap<String, String> {
        list.iter().map(|(i, n)| (i.to_string(), n.to_string())).collect()
    }

    fn fixture() -> TagIndex {
        // v1: a,b   v2: a,c   v3: b,c,d
        TagIndex::build(
            pairs(&[
                ("v1", "a"), ("v1", "b"),
                ("v2", "a"), ("v2", "c"),
                ("v3", "b"), ("v3", "c"), ("v3", "d"),
            ]),
            &names(&[("a", "Alpha"), ("b", "Beta"), ("c", "Gamma"), ("d", "Delta")]),
        )
    }

    fn ids(r: &FacetResult) -> Vec<(String, u32)> {
        r.tags.iter().map(|f| (f.id.clone(), f.count)).collect()
    }

    #[test]
    fn without_selection_counts_all_videos_per_tag() {
        let r = fixture().facets(&[], 20).unwrap();
        assert_eq!(r.matches, 3);
        assert_eq!(
            ids(&r),
            vec![("a".into(), 2), ("b".into(), 2), ("c".into(), 2), ("d".into(), 1)]
        );
    }

    #[test]
    fn one_selected_tag_narrows_to_its_videos() {
        let r = fixture().facets(&["a".into()], 20).unwrap();
        assert_eq!(r.matches, 2);
        assert_eq!(ids(&r), vec![("b".into(), 1), ("c".into(), 1)]);
    }

    #[test]
    fn several_selected_tags_are_combined_with_and() {
        let r = fixture().facets(&["b".into(), "c".into()], 20).unwrap();
        assert_eq!(r.matches, 1); // nur v3
        assert_eq!(ids(&r), vec![("d".into(), 1)]);
    }

    #[test]
    fn only_tags_with_at_least_one_result_are_offered() {
        // a und d zusammen: kein Video -> keine Facetten, Trefferzahl 0.
        let r = fixture().facets(&["a".into(), "d".into()], 20).unwrap();
        assert_eq!(r.matches, 0);
        assert!(r.tags.is_empty());
    }

    #[test]
    fn selected_tags_are_not_offered_again() {
        let r = fixture().facets(&["a".into()], 20).unwrap();
        assert!(r.tags.iter().all(|f| f.id != "a"));
    }

    #[test]
    fn top_limits_the_result() {
        let r = fixture().facets(&[], 2).unwrap();
        assert_eq!(r.tags.len(), 2);
    }

    #[test]
    fn unknown_tag_is_an_error() {
        let err = fixture().facets(&["zzz".into()], 20).unwrap_err();
        assert_eq!(err, FacetError::UnknownTag("zzz".into()));
    }

    #[test]
    fn duplicate_assignments_are_counted_once() {
        let idx = TagIndex::build(pairs(&[("v1", "a"), ("v1", "a")]), &HashMap::new());
        let r = idx.facets(&[], 20).unwrap();
        assert_eq!(ids(&r), vec![("a".into(), 1)]);
    }

    #[test]
    fn tag_name_falls_back_to_id() {
        let idx = TagIndex::build(pairs(&[("v1", "x")]), &HashMap::new());
        assert_eq!(idx.facets(&[], 20).unwrap().tags[0].name, "x");
    }

    fn with_videos() -> TagIndex {
        let mut idx = fixture();
        idx.attach_videos(vec![
            ("v1".into(), "Zebra".into(), Some(60.0)),
            ("v2".into(), "Apfel".into(), None),
            ("v3".into(), "Mango".into(), Some(120.5)),
            ("v4".into(), "Kiwi".into(), Some(30.0)), // ohne Tags
        ]);
        idx
    }

    #[test]
    fn videos_without_selection_include_untagged_and_sort_by_name() {
        let (total, rows) = with_videos().videos(&[], 0, 10).unwrap();
        assert_eq!(total, 4);
        let names: Vec<_> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["Apfel", "Kiwi", "Mango", "Zebra"]);
        assert_eq!(rows[0].id, "v2");
        assert_eq!(rows[2].duration_seconds, Some(120.5));
    }

    #[test]
    fn videos_with_selection_are_and_combined_and_sorted() {
        // b und c: nur v3 (Mango)
        let (total, rows) = with_videos().videos(&["b".into(), "c".into()], 0, 10).unwrap();
        assert_eq!(total, 1);
        assert_eq!(rows[0].name, "Mango");
    }

    #[test]
    fn videos_are_paged() {
        let idx = with_videos();
        let (total, rows) = idx.videos(&[], 1, 2).unwrap();
        assert_eq!(total, 4);
        let names: Vec<_> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["Kiwi", "Mango"]);
    }

    #[test]
    fn videos_unknown_tag_is_an_error() {
        assert!(with_videos().videos(&["zzz".into()], 0, 10).is_err());
    }

    #[test]
    fn facets_count_untagged_videos_in_total() {
        assert_eq!(with_videos().facets(&[], 20).unwrap().matches, 4);
    }

    /// Lasttest mit Produktivgroessenordnung: `cargo test -p video-store tag_index -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn benchmark_production_scale() {
        // 100.000 Videos, je 15 Tags aus 1,5 Mio Tags (deterministisch gestreut).
        let videos = 100_000u32;
        let tags = 1_500_000u32;
        let mut assignments = Vec::with_capacity((videos * 15) as usize);
        let mut seed: u64 = 42;
        for v in 0..videos {
            for _ in 0..15 {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let t = ((seed >> 33) % tags as u64) as u32;
                assignments.push((format!("v{v}"), format!("t{t}")));
            }
        }
        let t0 = std::time::Instant::now();
        let idx = TagIndex::build(assignments, &HashMap::new());
        println!("build: {:?} ({} videos, {} tags)", t0.elapsed(), idx.video_count(), idx.tag_count());

        let t1 = std::time::Instant::now();
        let r = idx.facets(&[], 20).unwrap();
        println!("facets ohne Auswahl: {:?}, top={}", t1.elapsed(), r.tags.len());

        let some = idx.tag_ids[0].clone();
        let t2 = std::time::Instant::now();
        let r = idx.facets(&[some.clone()], 20).unwrap();
        println!("facets eine Auswahl: {:?}, matches={}", t2.elapsed(), r.matches);

        let t3 = std::time::Instant::now();
        let r = idx.facets(&[some, idx.tag_ids[1].clone()], 20).unwrap();
        println!("facets zwei Auswahl: {:?}, matches={}", t3.elapsed(), r.matches);
    }
}
