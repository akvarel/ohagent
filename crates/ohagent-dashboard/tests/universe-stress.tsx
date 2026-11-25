// Synthetic renderer capacity fixture only. No runtime, auth or API injection.
import { useEffect, useState } from 'react'
import { createRoot } from 'react-dom/client'
import Universe from '../src/aaas/Universe'
import { translate } from '../src/aaas/i18n'
import type { Run } from '../src/aaas/types'
import '../src/aaas/aaas.css'
const tasks = Array.from({length:100}, (_,i) => ({id:`fixture-task-${i}`, agent_definition_id:`fixture-role-${i}`, agent_instance_id:`fixture-instance-${i}`, attempt_id:`fixture-attempt-${i}`, status:'running', summary:'Synthetic capacity fixture'}))
const base = {id:'fixture-run-0',process_id:'fixture-process',process_version:1,title:'Synthetic stress fixture',goal:'Renderer measurement only',status:'running',version:1,watermark:0,created_at:'',updated_at:'',tasks,edges:Array.from({length:200},(_,i) => ({id:`fixture-edge-${i}`,from:tasks[i%100].id,to:tasks[(i+1+Math.floor(i/100))%100].id,status:'dispatched'}))} as Run
const samples: number[] = []
let sentAt = performance.now()
let update: (sequence:number) => void = () => {}
Object.assign(window, { fixture: { samples, send(sequence:number) { sentAt=performance.now(); update(sequence) } } })
function Stress() {
  const [sequence,setSequence] = useState(0)
  update=setSequence
  useEffect(() => { const frame=requestAnimationFrame(() => samples.push(performance.now()-sentAt)); return () => cancelAnimationFrame(frame) },[sequence])
  return <main className="aaas-client"><div className="aaas-main"><h1>Synthetic renderer stress fixture</h1><p>No real runtime or durable transport. Ten fixture run summaries, selected graph: 100 nodes / 200 edges.</p><nav>{Array.from({length:10},(_,i)=><span key={i}> Fixture run {i+1}: running </span>)}</nav><output data-sequence={sequence}>{sequence}</output><Universe run={{...base,version:sequence+1,watermark:sequence}} pulses={[]} reduced={true} replay={true} t={key=>translate('en',key)} /></div></main>
}
createRoot(document.getElementById('root')!).render(<Stress />)
