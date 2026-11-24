// Development-only component acceptance. Never imported by the production app.
import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import Universe from '../src/aaas/Universe'
import { translate, type Language } from '../src/aaas/i18n'
import type { Run } from '../src/aaas/types'
import evidence from '../../../docs/evidence/aaas-001/live-runtime.json'
import '../src/aaas/aaas.css'

const trace = evidence.runs[0]
const snapshots = trace.events.map(event => event.payload.snapshot).filter(Boolean) as unknown as Run[]
function Acceptance() {
  const [position, setPosition] = useState(0)
  const [language, setLanguage] = useState<Language>('en')
  const run = snapshots[position]
  return <main className="aaas-client"><div className="aaas-main">
    <h1>Recorded real runtime · component acceptance</h1>
    <p>Read-only journal from actual Jcode/provider execution with private test identities. Not a live customer session or deployed OIDC evidence.</p>
    <label>Language<select value={language} onChange={event => setLanguage(event.target.value as Language)}><option>en</option><option>lv</option><option>ru</option></select></label>
    <label>Stored event<input aria-label="Stored event" type="range" min="0" max={snapshots.length - 1} value={position} onChange={event => setPosition(Number(event.target.value))} /></label>
    <output data-testid="position">{position + 1}/{snapshots.length}</output>
    <Universe run={run} pulses={[]} reduced={true} replay={true} t={key => translate(language, key)} />
    <h2>Actual verified result</h2><pre style={{whiteSpace:'pre-wrap',overflowWrap:'anywhere'}}>{trace.artifact_markdown}</pre>
  </div></main>
}
createRoot(document.getElementById('root')!).render(<Acceptance />)
