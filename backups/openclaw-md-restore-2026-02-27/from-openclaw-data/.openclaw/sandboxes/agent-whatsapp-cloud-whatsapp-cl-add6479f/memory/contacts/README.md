# Contacts Memory Schema

Un file per contatto: `+<numero>.md`
Campi consigliati:
- first_seen_at
- last_seen_at
- display_name
- role (`owner_admin`, `lead`, `active_client`)
- trust_level (`trusted_owner`, `standard_contact`)
- auth_mode (`phone_number_match`, `manual_review`)
- company_or_project
- lead_status (`new_lead`, `qualified`, `active_client`, `waiting_owner`, `closed`)
- requests
- urgency
- budget_range
- lead_source (`wellanet.dev`, `whatsapp_direct`, `referral`, altro)
- next_action
- owner_note
- notes_non_sensitive

Regole minime:
- Numero owner trusted: `+393471443005`, ruolo sempre `owner_admin`.
- Numero Marco: `+393924912119`, ruolo `active_client`.
- Non memorizzare dati personali non necessari (famiglia, salute, documenti, credenziali).
