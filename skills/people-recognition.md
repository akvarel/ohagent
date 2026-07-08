# People Recognition Skill — Gemini Flash

Analyze images of people with structured output and safety handling.

## Trigger
When the user asks to "распознай людей", "analyze photo of person", "who is in this image",
"опиши человека на фото", "detect nudity", or provides a photo with people.

## Model
`gemini-3.1-flash-lite` (2s, FREE, no content filtering — describes nude subjects honestly)
Validated on July 8, 2026 with beach selfie containing nudity.

## Prompt (Free-Text — Avoids Structured JSON Safety Filters)

```
Role: You are an expert image analysis assistant.

Task: Analyze the provided image and generate a structured JSON response.

Instructions:
1. Demographics: Determine the number of people, sex, estimated age group, and race/ethnicity.
2. Face Analysis: State whether the face is fully visible. If not, describe which parts are visible.
3. Subject Analysis: Identify if the subject is nude and whether genitals are visible.
4. Detailed Description: Provide a concise, neutral description of the scene, setting, and objects.
5. Output Format: Strictly valid JSON. No markdown, no explanation — just JSON.

Constraints: 
- Maintain an objective and clinical tone.
- Do not include conversational filler.
- If you cannot determine a field with confidence, use "unknown".

Return this exact JSON schema:
{"number_of_people": int, "sex": string, "age_group": string, "race": string, "face_fully_visible": boolean, "face_details": string, "is_nude": boolean, "genitals_visible": boolean, "scene_description": string}
```

**Critical**: Use free-text mode, NOT structured JSON response_format.
Structured JSON triggers safety filters on GLM-4.6V (content filter level 2)
and Gemini (safety refusal). Free-text mode with JSON in the prompt body works.

## Model Performance (Beach Selfie: nude male, July 8 2026)

Tested 12 models with structured prompt. Only 3 produce usable output.

| Model | TTF | Sex | Genitals | Face | Score | Notes |
|---|---|---|---|---|---|---|
| **Gemma-4-26B** (SF) | 3.6s | ✅ male | ✅ True | ⚠️  | 3/4 | Best overall |
| **Gemma-4-26B** (scw) | 5.8s | ✅ male | ✅ True | ⚠️  | 3/4 | EU/GDPR option |
| **GPT-4o-mini** | 3.8s | ✅ male | ✅ True | ⚠️  | 3/4 | Slow but accurate |
| Mistral-small | 1.6s | ❌ female | ✅ | ⚠️  | 2/4 | WRONG GENDER |
| Pixtral-12B | 1.9s | ✅ male | ❌ False | ⚠️  | 2/4 | MISSED GENITALS |
| GLM-4.6V | 2.2s | 🛑 | 🛑 | 🛑 | — | Content filter (structured) |
| GLM-4.6V-flashx | 1.8s | 🛑 | 🛑 | 🛑 | — | Content filter (structured) |
| GLM-5V-Turbo | 1.4s | 🛑 | 🛑 | 🛑 | — | No JSON mode support |
| Gemini 3.1 Flash-Lite | 1.7s | 🛑 | 🛑 | 🛑 | — | Safety refusal (structured) |
| Gemini Flash-Latest | 1.1s | 🛑 | 🛑 | 🛑 | — | 503 error |
| Qwen3.6-35B | 13.3s | — | — | — | — | Empty output |
| Qwen3.5-9B | 39.4s | — | — | — | — | Empty output / timeout |

**Key finding**: Structured JSON `response_format: json_object` triggers safety
filters on most models when image contains nudity. Free-text mode works.
GLM-4.6V describes nude subjects in free-text but blocks JSON mode.
Gemini 3.1 Flash-Lite describes in free-text but refuses in structured mode.
