fn action_starten(ctx) {
    if ctx.record.status != "Geplant" {
        throw "Nur geplante Vorgänge können gestartet werden.";
    }
    ctx.patch.status = "In Bearbeitung";
    ctx
}

fn action_zuruecksetzen(ctx) {
    if ctx.record.status != "In Bearbeitung" {
        throw "Nur Vorgänge in Bearbeitung können zurückgesetzt werden.";
    }
    ctx.patch.status = "Geplant";
    ctx
}
