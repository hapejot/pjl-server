fn before_create(ctx) {
    log("PoC-CRM: Account wird angelegt");
    ctx.payload.name = "Firmenname";
    ctx
}

fn before_patch(ctx) { log("Patch"); ctx }
fn before_delete(ctx) { log("Delete"); ctx }
fn before_draft_edit(ctx) { log("DraftEdit"); ctx }
fn before_draft_activate(ctx) { log("DraftActivate"); ctx }
fn before_draft_prepare(ctx) { 
    log("DraftPrepare"); 
    log(ctx);
    if ctx.current.draft.city != "Hannover" {throw "Stadt ist nicht Hannover";}
    ctx 
}
