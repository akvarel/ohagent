import { useState } from 'react';
import { Link } from 'react-router-dom';
import { useLocale } from './locale';
import { Arrow, Notice } from './PublicLayout';

export default function Demo() {
  const { t } = useLocale();
  const [step, setStep] = useState(0);
  const stages = [t.sampleStepOne, t.sampleStepTwo, t.sampleStepThree];
  return <section className="oh-wrap oh-section oh-demo">
    <Link className="oh-text-link oh-back" to="/agent-as-a-service">← {t.demoBack}</Link>
    <p className="oh-eyebrow"><span className="oh-square" />{t.demoBadge}</p><h1>{t.demoTitle}</h1><p className="oh-lead">{t.demoIntro}</p>
    <div className="oh-demo-workspace"><div className="oh-demo-definition"><span className="oh-tag">{t.definition} / {t.sample}</span><h2>{t.sampleAgent}</h2><p>{t.sampleDefinition}</p><div className="oh-demo-task"><small>{t.sample}</small><strong>{t.sampleTask}</strong></div><button className="oh-button oh-button-primary" onClick={() => setStep(current => Math.min(current + 1, 3))} disabled={step === 3}>{step === 0 ? t.demoStart : step === 3 ? t.sampleComplete : t.demoNext}<Arrow /></button><button className="oh-text-link oh-reset" onClick={() => setStep(0)} disabled={step === 0}>{t.demoReset}</button></div>
      <div className="oh-demo-output"><p className="oh-eyebrow">{t.instance} / {t.sample}</p><ol className="oh-demo-stages">{stages.map((stage, index) => <li className={step > index ? 'is-complete' : ''} key={stage}><span aria-hidden="true">{step > index ? '✓' : index + 1}</span><p>{stage}</p></li>)}</ol><div className="oh-demo-result" role="status" aria-live="polite"><strong>{step === 3 ? t.sampleComplete : step === 0 ? t.sampleReady : stages[step - 1]}</strong>{step === 3 && <p>{t.sampleResult}</p>}</div></div>
    </div>
    <Notice>{t.demoDisclaimer}</Notice>
  </section>;
}
