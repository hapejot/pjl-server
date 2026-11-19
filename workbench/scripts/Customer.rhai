fn before_create(ctx) {
	ctx.payload.industry = ctx.payload.name.to_upper();
	ctx
}
