// Komponente TimeRange: Beginn, Ende und Dauer eines Zeitraums konsistent halten.
// Feldnamen und Rasterung kommen aus der DDIC (cfg), nicht aus dem Skript.
//
// Regeln (Vorrang von oben nach unten):
//   1. Ende geaendert    -> Dauer = Ende - Beginn
//   2. Dauer geaendert   -> Ende  = Beginn + Dauer
//   3. nur Beginn        -> Dauer bleibt, Ende wird verschoben
// Fehlt ein Wert im Patch, wird der gespeicherte Wert aus ctx.current genommen.
// Zeitpunkte werden auf cfg.step_minutes gerundet, Dauern auf ganze Schritte.

fn has(m, k) {
    type_of(m) == "map" && k in m && m[k] != ()
}

// Wert aus dem Patch, sonst aus dem Bestand, sonst ().
fn pick(p, cur, k) {
    if has(p, k) { p[k] } else if has(cur, k) { cur[k] } else { () }
}

fn before_create(ctx, cfg) {
    apply_rules(ctx, cfg)
}

fn before_patch(ctx, cfg) {
    apply_rules(ctx, cfg)
}

fn apply_rules(ctx, cfg) {
    let f = cfg.fields;
    let p = ctx.payload;
    let cur = ctx.current;
    if has(p, f.start) {
        p[f.start] = ts_round_minutes(p[f.start], cfg.step_minutes);
    }
    if has(p, f.end) {
        p = rule_end_changed(p, cur, f, cfg.step_minutes);
    } else if has(p, f.duration) {
        p = rule_duration_changed(p, cur, f, cfg.step_minutes);
    } else if has(p, f.start) {
        p = rule_start_changed(p, cur, f, cfg.step_minutes);
    }
    ctx.payload = p;
    ctx
}

// Regel 1: Ende eingegeben, Dauer ableiten.
fn rule_end_changed(p, cur, f, step) {
    let start = pick(p, cur, f.start);
    p[f.end] = ts_round_minutes(p[f.end], step);
    if start != () {
        let d = ts_diff_minutes(start, p[f.end]);
        if d < 0 {
            throw "Das Ende liegt vor dem Beginn";
        }
        p[f.duration] = d;
    }
    p
}

// Regel 2: Dauer eingegeben, Ende berechnen.
fn rule_duration_changed(p, cur, f, step) {
    let start = pick(p, cur, f.start);
    if p[f.duration] < 0 {
        throw "Die Dauer darf nicht negativ sein";
    }
    p[f.duration] = (p[f.duration] / step) * step;
    if start != () {
        p[f.end] = ts_add_minutes(start, p[f.duration]);
    }
    p
}

// Regel 3: nur Beginn. Dauer aus dem Bestand uebernehmen.
fn rule_start_changed(p, cur, f, step) {
    let dur = duration_from_current(cur, f);
    if dur != () {
        p[f.duration] = dur;
        p[f.end] = ts_add_minutes(p[f.start], dur);
    }
    p
}

fn duration_from_current(cur, f) {
    if has(cur, f.duration) {
        cur[f.duration]
    } else if has(cur, f.start) && has(cur, f.end) {
        ts_diff_minutes(cur[f.start], cur[f.end])
    } else {
        ()
    }
}
