let texts = observation_texts(ctx, "extracted_text");
if texts.len > 0 {
    if texts[0].contains("standing data") || texts[0].contains("payment retries") {
        emit_claim(
            ctx,
            "concept:acme:latent-knowledge",
            "semantic:may_depend_on",
            "view:acme:cross-document-correlation",
            "ctx:acme",
            0.61
        );
        emit_note(ctx, "latent dependency hint observed in source text");
    }
}
