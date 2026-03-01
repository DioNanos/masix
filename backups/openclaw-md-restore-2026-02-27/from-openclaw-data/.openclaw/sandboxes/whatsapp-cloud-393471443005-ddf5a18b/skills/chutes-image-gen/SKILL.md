---
name: chutes-image-gen
description: Generate and edit images using Chutes.ai Qwen-Image models. Features batch mode with random prompts, seed-based reproducibility, and automatic gallery organization.
homepage: https://chutes.ai
metadata:
  {
    "openclaw":
      {
        "emoji": "🎨",
        "requires": { "bins": ["python3"], "env": ["CHUTES_API_TOKEN"] },
        "primaryEnv": "CHUTES_API_TOKEN",
      },
  }
---

# Chutes Image Generation

Generate and edit images using Chutes.ai Qwen-Image models via their custom API.

## Models

Text-to-image generation models:
- **Qwen-Image-2512** (alias: `qwen`) - Default, excellent for descriptive prompts
- **Z-Image-Turbo** (alias: `turbo`) - Fast generation
- **HiDream** (alias: `hidream`) - High quality artistic
- **Hunyuan-Image-3** (alias: `hunyuan`) - Tencent's model
- **Wan2.1-14b** (alias: `wan`) - Large scale model
- **FLUX.1-Dev** (alias: `flux`) - Black Forest Labs

Image editing:
- **Qwen-Image-Edit-2509** - Edit existing images with instructions

## Run

```bash
# Generate single image
python3 {baseDir}/scripts/generate.py --prompt "A beautiful sunset over mountains"

# Generate with custom parameters
python3 {baseDir}/scripts/generate.py --prompt "Cyberpunk city at night" --width 1024 --height 1024 --steps 50

# Batch generation (random prompts)
python3 {baseDir}/scripts/generate.py --batch 5 --out-dir ./my-images

# Edit existing image
python3 {baseDir}/scripts/edit.py --prompt "Add a rainbow in the sky" --image /path/to/image.png
```

## Examples

```bash
# Quick generation with defaults (Qwen)
python3 {baseDir}/scripts/generate.py --prompt "Lobster astronaut on the moon"

# Use FLUX model
python3 {baseDir}/scripts/generate.py --prompt "Cyberpunk city" --model flux

# Fast generation with Turbo
python3 {baseDir}/scripts/generate.py --prompt "Quick sketch" --model turbo

# High quality generation
python3 {baseDir}/scripts/generate.py --prompt "Detailed fantasy landscape" --steps 70 --guidance 8.5

# Reproducible with seed
python3 {baseDir}/scripts/generate.py --prompt "Mountain lake reflection" --seed 42

# Batch mode (generates 5 random prompts)
python3 {baseDir}/scripts/generate.py --batch 5

# Custom output directory
python3 {baseDir}/scripts/generate.py --prompt "Steampunk owl" --out-dir ~/Pictures/ai-art
```

## Features

### Single Image Mode
- Custom prompts with full parameter control
- Seed-based reproducibility
- Negative prompts for fine-tuning

### Batch Mode
- Random prompt generation from curated templates
- Automatic output directory management
- Metadata JSON with prompt→file mapping
- Perfect for inspiration and exploration

### Prompt Templates
Includes 120+ curated prompt combinations:
- 12 subjects (lobster astronaut, cyberpunk city, fantasy castle, etc.)
- 10 styles (photorealistic, oil painting, anime, 3D render, etc.)
- 8 lighting options (golden hour, neon, volumetric fog, etc.)

## Output

- `YYYYMMDD_HHMMSS_<slug>.png` - Generated images
- `prompts.json` - Metadata (batch mode only)
- Base64 encoded image returned to OpenClaw for display

## Parameters

### Generation

| Parameter | Default | Description |
|-----------|---------|-------------|
| `--prompt` | (required) | Text prompt for image generation |
| `--negative-prompt` | "" | What to avoid in the image |
| `--width` | 1024 | Image width (px) |
| `--height` | 1024 | Image height (px) |
| `--steps` | 50 | Inference steps (higher = better quality, slower) |
| `--guidance` | 7.5 | How closely to follow prompt (1-20) |
| `--seed` | random | Seed for reproducibility |
| `--model` | qwen | Model alias (qwen, turbo, hidream, hunyuan, wan, flux) or full name |
| `--batch` | - | Generate multiple random images |
| `--out-dir` | ./tmp/... | Output directory |

### Editing

| Parameter | Default | Description |
|-----------|---------|-------------|
| `--prompt` | (required) | Edit instruction |
| `--image` | (required) | Input image path |
| `--steps` | 40 | Inference steps |
| `--cfg` | 4.0 | CFG scale |

## Tips

### Quality Settings

**Fast (draft):**
```bash
--steps 30 --guidance 6.5
```

**Standard:**
```bash
--steps 50 --guidance 7.5
```

**High Quality:**
```bash
--steps 70 --guidance 8.5
```

### Prompt Engineering

**Good prompts are specific:**
```
✅ "ultra-detailed photorealistic portrait of a wise wizard with long white beard, 
     wearing starry blue robe, holding glowing crystal staff, golden hour lighting"

❌ "wizard picture"
```

**Use negative prompts to avoid issues:**
```
--negative-prompt "blurry, low quality, deformed, ugly, bad anatomy, watermark, text"
```

### Reproducibility

Save seeds for variations:
```bash
# Generate base image
python3 {baseDir}/scripts/generate.py --prompt "Mountain lake" --seed 42

# Try variations with same composition
python3 {baseDir}/scripts/generate.py --prompt "Mountain lake at sunset" --seed 42
python3 {baseDir}/scripts/generate.py --prompt "Mountain lake in winter" --seed 42
```

## Troubleshooting

### ERROR: CHUTES_API_TOKEN not set
Add to `~/.openclaw/secrets.env`:
```bash
CHUTES_API_TOKEN=your_token_here
```

### HTTP Error 401
- Token is invalid or expired
- Check CHUTES_API_TOKEN value

### HTTP Error 429 (Rate Limited)
- Wait a few minutes before retrying
- Chutes may have rate limits

### HTTP Error 500
- Temporary server issue
- Retry with same parameters

### Low Quality Images
- Increase `--steps` to 60-70
- Increase `--guidance` to 8-9
- Use more detailed prompts

## Integration with OpenClaw

Once installed, simply ask:

```
"Genera un'immagine di un tramonto sulle montagne"
"Crea un'immagine cyberpunk di una città futuristica"
"Modifica questa immagine aggiungendo un arcobaleno"
```

OpenClaw will automatically use the Chutes Image skill.
