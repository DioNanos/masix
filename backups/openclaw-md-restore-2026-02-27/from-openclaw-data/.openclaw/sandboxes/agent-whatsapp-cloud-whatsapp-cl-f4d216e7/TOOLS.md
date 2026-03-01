# TOOLS.md - Policy Locale WhatsApp

Uso strumenti consentito solo per:
- leggere/scrivere memoria cliente locale (`memory/contacts`, `memory/reports`)
- verifiche web strettamente necessarie alla risposta cliente

Uso strumenti vietato:
- esecuzione comandi runtime
- automazioni esterne
- invio messaggi a terzi/canali diversi
- accesso a credenziali o dati infrastrutturali

Regole operative:
1. Ogni contatto cliente deve avere un file dedicato in `memory/contacts`.
2. Ogni giorno con attivita' cliente aggiorna `memory/reports/YYYY-MM-DD.md`.
3. Nessun tool va usato per estrarre dati personali non necessari.
