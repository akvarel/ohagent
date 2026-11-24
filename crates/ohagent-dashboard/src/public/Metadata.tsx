import { useEffect } from 'react';
import { useLocation } from 'react-router-dom';
import { useLocale } from './locale';

export default function Metadata() {
  const { pathname } = useLocation();
  const { language, t } = useLocale();
  useEffect(() => {
    const isHome = pathname === '/';
    const isProduct = pathname === '/agent-as-a-service' || pathname === '/agent-as-a-service/';
    const isDemo = pathname.startsWith('/agent-as-a-service/demo');
    const isApp = pathname.startsWith('/agent-as-a-service/app');
    const isOperator = /^\/(operator|progress|skills|memory|keys|usage)(\/|$)/.test(pathname);
    const indexable = isHome || isProduct;
    const title = isHome ? 'OrangeHat | ' + t.hero.replace('\n', ' ') : isProduct ? t.product + ' | OrangeHat' : isDemo ? t.demoBadge + ' | OrangeHat' : isApp ? t.openApp + ' | ' + t.product : isOperator ? 'ohAgent | ' + t.operator : t.notFound + ' | OrangeHat';
    const description = isHome ? t.intro : isProduct || isApp ? t.productIntro : isDemo ? t.demoIntro : isOperator ? t.operatorNote : t.notFoundBody;
    document.title = title;
    document.documentElement.lang = language;
    function meta(name: string, value: string, property = false) {
      const attribute = property ? 'property' : 'name';
      let node = document.head.querySelector<HTMLMetaElement>(`meta[${attribute}="${name}"]`);
      if (!node) { node = document.createElement('meta'); node.setAttribute(attribute, name); document.head.append(node); }
      node.content = value;
    }
    meta('description', description);
    meta('robots', indexable ? 'index, follow' : 'noindex, nofollow');
    meta('og:title', title, true);
    meta('og:description', description, true);
    meta('og:type', 'website', true);
    meta('og:locale', { en: 'en_US', lv: 'lv_LV', ru: 'ru_RU' }[language], true);
    meta('twitter:card', 'summary');
    meta('twitter:title', title);
    meta('twitter:description', description);
    const canonicalUrl = new URL(isProduct ? '/agent-as-a-service' : pathname, window.location.origin).href;
    meta('og:url', canonicalUrl, true);
    let canonical = document.head.querySelector<HTMLLinkElement>('link[rel="canonical"]');
    if (!canonical) { canonical = document.createElement('link'); canonical.rel = 'canonical'; document.head.append(canonical); }
    canonical.href = canonicalUrl;
    let data = document.getElementById('public-structured-data');
    if (indexable) {
      if (!data) { data = document.createElement('script'); data.id = 'public-structured-data'; data.setAttribute('type', 'application/ld+json'); document.head.append(data); }
      data.textContent = JSON.stringify({ '@context': 'https://schema.org', '@type': 'WebPage', name: title, description, url: canonicalUrl, inLanguage: language, isPartOf: { '@type': 'WebSite', name: 'OrangeHat', url: window.location.origin } });
    } else { data?.remove(); }
  }, [pathname, language, t]);
  return null;
}
