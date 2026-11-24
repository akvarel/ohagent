import { Link } from 'react-router-dom';
import { productCatalog } from './catalog';
import { useLocale } from './locale';
import { Architecture, Arrow, Buttons, Notice, SectionIntro } from './PublicLayout';

export default function Home() {
  const { t, language } = useLocale();
  return <>
    <section className="oh-wrap oh-hero">
      <div className="oh-hero-copy"><p className="oh-eyebrow"><span className="oh-square" />{t.eyebrow}</p><h1>{t.hero}</h1><p className="oh-lead">{t.intro}</p><Buttons /><p className="oh-caption">{t.heroNote}</p></div>
      <Architecture compact />
    </section>
    <section id="ecosystem" className="oh-catalog-section"><div className="oh-wrap oh-section">
      <SectionIntro eyebrow={t.catalog} title={t.catalogTitle} body={t.catalogIntro} />
      <div className="oh-product-grid">{productCatalog.filter(entry => entry.publish).map(entry => <article className={`oh-product-card oh-product-${entry.id}`} key={entry.id}>
        <div className="oh-card-top"><span className="oh-tag">{t[entry.categoryKey]}</span><span className="oh-product-mark" aria-hidden="true">{entry.id === 'ohagent' ? 'oh' : entry.id === 'bugzero' ? 'b₀' : 'g↗'}</span></div>
        <h3>{entry.displayName}</h3><p>{t[entry.summaryKey]}</p><p>{t[entry.targetUserKey]}</p><p className="oh-caption">{t[entry.statusKey]}</p>
        <div className="oh-card-links">{entry.primaryUrl.startsWith('/') ? <Link className="oh-text-link" to={entry.primaryUrl}>{t[entry.ctaLabelKey]}<Arrow /></Link> : <a className="oh-text-link" href={entry.primaryUrl} target="_blank" rel="noopener noreferrer">{t[entry.ctaLabelKey]}<Arrow /></a>}{entry.primaryUrl !== entry.evidence.href && <a className="oh-source-link" href={entry.evidence.href} target="_blank" rel="noopener noreferrer">{t.internalEvidence}<Arrow /></a>}</div>
        <small>{t.checked}: <time dateTime={entry.evidence.reviewedOn}>{new Intl.DateTimeFormat(language, { dateStyle: 'medium', timeZone: 'UTC' }).format(new Date(entry.evidence.reviewedOn))}</time></small>
      </article>)}</div>
      <p className="oh-evidence-note">{t.evidenceNote}</p>
    </div></section>
    <section id="approach" className="oh-wrap oh-section">
      <div className="oh-split-intro"><SectionIntro eyebrow={t.approach} title={t.approachTitle} /><p className="oh-lead">{t.approachIntro}</p></div>
      <div className="oh-process-grid">{[[t.define, t.defineBody], [t.compose, t.composeBody], [t.run, t.runBody]].map(([title, body], index) => <article className="oh-process" key={title}><span className="oh-step">0{index + 1}</span><h3>{title}</h3><p>{body}</p></article>)}</div>
      <Notice>{t.standardNote}</Notice>
    </section>
    <section id="contact" className="oh-contact"><div className="oh-wrap oh-section oh-contact-grid"><div><p className="oh-eyebrow">{t.contact}</p><h2>{t.contactTitle}</h2><p className="oh-lead">{t.contactBody}</p><Link to="/agent-as-a-service" className="oh-button oh-button-primary">{t.explore}<Arrow /></Link></div><div className="oh-contact-note"><span className="oh-contact-symbol" aria-hidden="true">↗</span><p>{t.contactUnavailable}</p><small>{t.availability}</small></div></div></section>
  </>;
}
