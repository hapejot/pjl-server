// Termin: Beginn, Ende und Dauer (Minuten) konsistent halten.
// Zeitpunkte werden auf 5-Minuten-Schritte gerundet.
//
// Regeln beim Patch (Vorrang von oben nach unten):
//   1. Ende geaendert   -> Dauer = Ende - Beginn
//   2. Dauer geaendert  -> Ende  = Beginn + Dauer
//   3. nur Beginn       -> Dauer bleibt, Ende wird verschoben
// Fehlt der Beginn im Patch, wird der gespeicherte Wert aus ctx.current genommen.

fn step_min() { 5 }

fn has(m, k) {
    k in m && m[k] != ()
}

fn before_patch(ctx) {
    let p = ctx.payload;
    let cur = ctx.current;

    let start = if has(p, "starts_at") { p.starts_at } else if has(cur, "starts_at") { cur.starts_at } else { () };
    let end = if has(p, "ends_at") { p.ends_at } else if has(cur, "ends_at") { cur.ends_at } else { () };

    if has(p, "starts_at") {
        p.starts_at = ts_round_minutes(p.starts_at, step_min());
        start = p.starts_at;
    }

    if has(p, "ends_at") {
        // Regel 1: Endzeit eingegeben, Dauer ableiten.
        p.ends_at = ts_round_minutes(p.ends_at, step_min());
        end = p.ends_at;
        if start != () {
            let d = ts_diff_minutes(start, end);
            if d < 0 {
                throw "Das Ende liegt vor dem Beginn";
            }
            p.duration_minutes = d;
        }
    } else if has(p, "duration_minutes") {
        // Regel 2: Dauer eingegeben, Ende berechnen.
        if p.duration_minutes < 0 {
            throw "Die Dauer darf nicht negativ sein";
        }
        p.duration_minutes = (p.duration_minutes / step_min()) * step_min();
        if start != () {
            p.ends_at = ts_add_minutes(start, p.duration_minutes);
        }
    } else if has(p, "starts_at") {
        // Regel 3: nur Beginn geaendert. Dauer aus dem Bestand uebernehmen.
        let dur = ();
        if has(cur, "duration_minutes") {
            dur = cur.duration_minutes;
        } else if has(cur, "starts_at") && has(cur, "ends_at") {
            dur = ts_diff_minutes(cur.starts_at, cur.ends_at);
        }
        if dur != () {
            p.duration_minutes = dur;
            p.ends_at = ts_add_minutes(start, dur);
        }
    }

    ctx.payload = p;
    ctx
}
