fn before_patch(ctx) {
        let industry = ctx.payload.industry;
        if industry != () {
                ctx.payload.industry = industry.to_upper();
        }
        ctx
}
