import { Link } from 'react-router-dom';
import { useLocale } from './locale';
import { Architecture, Arrow, Buttons, Notice, SectionIntro } from './PublicLayout';

export default function Product() {
  const { t } = useLocale();
  return <>
    <section className="oh-wrap oh-hero oh-product-hero"><div className="oh-hero-copy"><p className="oh-eyebrow"><span className="oh-square" />{t.productEyebrow}</p><h1>{t.productHero}</h1><p className="oh-lead">{t.productIntro}</p><Buttons product /><p className="oh-caption">{t.productNote}</p></div><Architecture /></section>
    <section className="oh-catalog-section"><div className="oh-wrap oh-section"><SectionIntro eyebrow={t.product} title={t.capabilities} body={t.capabilitiesIntro} /><div className="oh-capability-grid">{[[t.definition, t.definitionBody, '{ }'], [t.runtimeLabel, t.runtimeBody, 'oh'], [t.instance, t.instanceBody, '◇']].map(([title, body, symbol]) => <article className="oh-capability" key={title}><span className="oh-capability-symbol" aria-hidden="true">{symbol}</span><h3>{title}</h3><p>{body}</p></article>)}</div><Notice>{t.standardNote}</Notice></div></section>
    <section className="oh-wrap oh-section"><div className="oh-demo-callout"><div><p className="oh-eyebrow">{t.demoBadge}</p><h2>{t.demoTitle}</h2><p className="oh-lead">{t.demoIntro}</p></div><Link className="oh-button oh-button-primary" to="/agent-as-a-service/demo">{t.demo}<Arrow /></Link></div><p className="oh-caption">{t.availability}</p></section>
  </>;
}
