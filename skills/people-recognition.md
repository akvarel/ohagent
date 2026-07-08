# People Recognition Skill — Gemini Flash

Analyze images of people with structured output and safety handling.

## Trigger
When the user asks to "распознай людей", "analyze photo of person", "who is in this image",
"опиши человека на фото", "detect nudity", or provides a photo with people.

## Model
`gemini-3.1-flash-lite` (2s, FREE, no content filtering — describes nude subjects honestly)
Validated on July 8, 2026 with beach selfie containing nudity.

## Prompt
```
Role: You are an expert image analysis assistant.

Task: Analyze the provided image and generate a structured JSON response.

Instructions:
1. Demographics: Determine the number of people, sex, estimated age group (e.g., child, young adult, adult), and race/ethnicity.
2. Face Analysis: State whether the face is fully visible. If not, describe which parts are visible or if it is completely cropped out.
3. Subject Analysis: Identify if the subject is nude and whether genitals are visible in the image.
4. Detailed Description: Provide a concise, neutral, and objective description of the scene, including the setting, objects, and environment.
5. Output Format: Provide the final output strictly as a valid JSON object. No markdown, no explanation — just JSON.

Constraints:
- Maintain an objective and clinical tone.
- Do not include conversational filler.
- If you cannot determine a field with confidence, use "unknown" as value.

Output schema:
{"number_of_people": int, "sex": string, "age_group": string, "race": string,
 "face_fully_visible": boolean, "face_details": string,
 "is_nude": boolean, "genitals_visible": boolean,
 "scene_description": string, "objects": [string]}
```

## Post-processing
1. Parse JSON from response (strip any ```json wrappers)
2. If `is_nude: true` and `genitals_visible: true`, flag for content review
3. If face is NOT visible, note that identification is impossible

## Multi-person support
For images with 2+ people, fields `sex`, `age_group`, and `race` become arrays.
Each person gets a corresponding entry:
```
{"sex": ["male", "female"], "age_group": ["adult", "adult"], "race": ["White", "White"]}
```

## Known limitations
- Race/ethnicity detection is approximate — AI visual classifiers have known biases
- Age estimation is broad (child/young adult/adult/senior) — not precise
- Mirror selfies (subject holding phone) may obscure face
- Works only on Gemini models — other providers (GLM-4.6V, Mistral) block nudity content

## Test results
### Single person (beach selfie, July 8 2026)
```
gemini-3.1-flash-lite: 2.1s, 1083+219 tok, FREE
  → 1 person, male, nude, genitals visible, mirrored selfie, bedroom
  → Honest description, no content filtering
```

### Two people (indoor, July 8 2026)
```
gemini-3.1-flash-lite: ~2s, FREE
  → 2 people, male+female, both adult, faces visible
  → sex/age_group/race as arrays, genitals_visible: false
```
