# Chutes Image Generation Skill for OpenClaw

Genera e modifica immagini usando i modelli Qwen-Image di Chutes.ai

## Installazione

Lo skill è già installato in: `/home/dag/.openclaw/skills/chutes-image-gen/`

## Requisiti

- `CHUTES_API_TOKEN` impostato in `~/.openclaw/secrets.env`
- Python 3
- `curl` e `jq` (per eventuali debug)

## Utilizzo da OpenClaw

### Generazione Immagini

Chiedi all'agente:
```
Genera un'immagine di un tramonto sulle montagne
Crea un'immagine cyberpunk di una città futuristica
Disegna un paesaggio fantasy con draghi
```

### Modifica Immagini

```
Modifica questa immagine aggiungendo un arcobaleno
Rendi questa immagine più colorata
Cambia lo sfondo in un tramonto
```

## Utilizzo Diretto (CLI)

### Generazione

```bash
# Base
python3 ~/.openclaw/skills/chutes-image-gen/scripts/generate.py --prompt "A beautiful sunset"

# Avanzato
python3 ~/.openclaw/skills/chutes-image-gen/scripts/generate.py \
  --prompt "Cyberpunk city at night" \
  --width 1024 \
  --height 1024 \
  --steps 50 \
  --guidance 7.5 \
  --seed 42
```

### Modifica

```bash
python3 ~/.openclaw/skills/chutes-image-gen/scripts/edit.py \
  --prompt "Add a rainbow in the sky" \
  --image /path/to/image.png
```

## Parametri

### Generazione

| Parametro | Default | Descrizione |
|-----------|---------|-------------|
| `--prompt` | (richiesto) | Descrizione testo dell'immagine |
| `--negative-prompt` | "" | Cosa evitare nell'immagine |
| `--width` | 1024 | Larghezza immagine |
| `--height` | 1024 | Altezza immagine |
| `--steps` | 50 | Passi di inferenza (più alti = più qualità) |
| `--guidance` | 7.5 | Quanto seguire il prompt (1-20) |
| `--seed` | random | Seed per riproducibilità |
| `--model` | Qwen-Image-2512 | Modello da usare |

### Modifica

| Parametro | Default | Descrizione |
|-----------|---------|-------------|
| `--prompt` | (richiesto) | Istruzione di modifica |
| `--image` | (richiesto) | Percorso immagine da modificare |
| `--steps` | 40 | Passi di inferenza |
| `--cfg` | 4.0 | CFG scale |
| `--seed` | random | Seed per riproducibilità |

## Output

- Immagine salvata in: `chutes_image_YYYYMMDD_HHMMSS.png`
- Immagine editata: `chutes_edited_YYYYMMDD_HHMMSS.png`
- Base64 dell'immagine stampato per visualizzazione in OpenClaw

## Modelli Disponibili

- **Qwen-Image-2512**: Generazione da testo (ottimo per prompt descrittivi)
- **Qwen-Image-Edit-2509**: Modifica immagini esistenti

## Esempi di Prompt

### Paesaggi
```
"A serene mountain lake at sunrise with mist, photorealistic, 8k"
"Fantasy forest with glowing mushrooms and fireflies, digital art"
```

### Personaggi
```
"Portrait of a wise wizard with long white beard, fantasy art style"
"Cyberpunk hacker with neon implants, dramatic lighting"
```

### Oggetti
```
"Ancient book with golden runes, leather bound, magical glow"
"Futuristic spaceship with blue ion engines, space background"
```

## Risoluzione Problemi

### Errore: CHUTES_API_TOKEN not set
Aggiungi in `~/.openclaw/secrets.env`:
```bash
CHUTES_API_TOKEN=cpk_ccc130d07ddb43ee9e9e31fc21c39e31.acadf82119c756449ee12872cd590799.sQto4rtVVVHsrRdoE5dGeUgpeiYqvaPO
```

### Errore HTTP 4xx
- Verifica che il token sia corretto
- Controlla i limiti della API

### Errore HTTP 5xx
- Il servizio Chutes potrebbe essere temporaneamente down
- Riprova tra qualche minuto

### Immagini di bassa qualità
- Aumenta `--steps` a 50-70
- Aumenta `--guidance` a 8-9
- Usa prompt più dettagliati e specifici

## License

Skill custom per uso personale con OpenClaw.
