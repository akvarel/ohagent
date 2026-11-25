import type { Copy } from './locale';

export interface ProductEvidence {
  kind: 'repository' | 'public-site' | 'public-repository' | 'project-documentation';
  href?: string;
  reviewedOn: string;
  scope: 'description-only';
}
export interface ProductEntry {
  id: 'ohagent' | 'bugzero' | 'celotleti' | 'flatly' | 'accounting' | 'medicine-reminder' | 'karmina' | 'graphify';
  displayName: string;
  mark: string;
  kind: 'runtime' | 'product' | 'reference' | 'technology';
  categoryKey: keyof Copy;
  summaryKey: keyof Copy;
  targetUserKey: keyof Copy;
  verifiedStatus: 'pilot-code-only' | 'availability-unverified' | 'public-source';
  statusKey: keyof Copy;
  relationship: 'orangehat-runtime' | 'orangehat-product' | 'client-reference' | 'public-product-reference' | 'third-party-tooling';
  primaryUrl?: string;
  ctaLabelKey: keyof Copy;
  publish: boolean;
  evidence: ProductEvidence;
}
// Review verifies descriptions, not production readiness or ohAgent integration.
// Products without a verified public URL remain visible without a fabricated link.
export const productCatalog: readonly ProductEntry[] = [
  {
    id: 'ohagent', displayName: 'ohAgent', mark: 'oh', kind: 'runtime', categoryKey: 'runtime', summaryKey: 'ohDescription', targetUserKey: 'ohUseCase',
    verifiedStatus: 'pilot-code-only', statusKey: 'pilotCode', relationship: 'orangehat-runtime', primaryUrl: '/agent-as-a-service', ctaLabelKey: 'learnMore', publish: true,
    evidence: { kind: 'repository', href: 'https://github.com/akvarel/ohagent', reviewedOn: '2026-11-24', scope: 'description-only' },
  },
  {
    id: 'bugzero', displayName: 'BugZero', mark: 'b₀', kind: 'product', categoryKey: 'publicProduct', summaryKey: 'bugDescription', targetUserKey: 'bugUseCase',
    verifiedStatus: 'availability-unverified', statusKey: 'availabilityUnverified', relationship: 'public-product-reference', primaryUrl: 'https://bugzero.lv/', ctaLabelKey: 'publicEvidence', publish: true,
    evidence: { kind: 'public-site', href: 'https://bugzero.lv/', reviewedOn: '2026-11-24', scope: 'description-only' },
  },
  {
    id: 'celotleti', displayName: 'Celotleti.lv', mark: 'c↗', kind: 'reference', categoryKey: 'clientReference', summaryKey: 'celotletiDescription', targetUserKey: 'celotletiUseCase',
    verifiedStatus: 'availability-unverified', statusKey: 'availabilityUnverified', relationship: 'client-reference', primaryUrl: 'https://celotleti.lv/', ctaLabelKey: 'visitWebsite', publish: true,
    evidence: { kind: 'public-site', href: 'https://celotleti.lv/', reviewedOn: '2026-11-25', scope: 'description-only' },
  },
  {
    id: 'flatly', displayName: 'Flatly.lv', mark: 'fl', kind: 'product', categoryKey: 'publicProduct', summaryKey: 'flatlyDescription', targetUserKey: 'flatlyUseCase',
    verifiedStatus: 'availability-unverified', statusKey: 'availabilityUnverified', relationship: 'orangehat-product', primaryUrl: 'https://flatly.lv/', ctaLabelKey: 'visitWebsite', publish: true,
    evidence: { kind: 'public-site', href: 'https://flatly.lv/', reviewedOn: '2026-11-25', scope: 'description-only' },
  },
  {
    id: 'accounting', displayName: 'Accounting', mark: 'ac', kind: 'product', categoryKey: 'portfolioProduct', summaryKey: 'accountingDescription', targetUserKey: 'accountingUseCase',
    verifiedStatus: 'availability-unverified', statusKey: 'availabilityUnverified', relationship: 'orangehat-product', ctaLabelKey: 'websiteNotPublished', publish: true,
    evidence: { kind: 'project-documentation', reviewedOn: '2026-11-25', scope: 'description-only' },
  },
  {
    id: 'medicine-reminder', displayName: 'Medicine Reminder', mark: 'm+', kind: 'product', categoryKey: 'portfolioProduct', summaryKey: 'medicineDescription', targetUserKey: 'medicineUseCase',
    verifiedStatus: 'availability-unverified', statusKey: 'availabilityUnverified', relationship: 'orangehat-product', ctaLabelKey: 'websiteNotPublished', publish: true,
    evidence: { kind: 'project-documentation', reviewedOn: '2026-11-25', scope: 'description-only' },
  },
  {
    id: 'karmina', displayName: 'Karmina', mark: 'ka', kind: 'product', categoryKey: 'portfolioProduct', summaryKey: 'karminaDescription', targetUserKey: 'karminaUseCase',
    verifiedStatus: 'availability-unverified', statusKey: 'availabilityUnverified', relationship: 'orangehat-product', ctaLabelKey: 'websiteNotPublished', publish: true,
    evidence: { kind: 'project-documentation', reviewedOn: '2026-11-25', scope: 'description-only' },
  },
  {
    id: 'graphify', displayName: 'Graphify', mark: 'g↗', kind: 'technology', categoryKey: 'openSource', summaryKey: 'graphDescription', targetUserKey: 'graphUseCase',
    verifiedStatus: 'public-source', statusKey: 'sourceAvailable', relationship: 'third-party-tooling', primaryUrl: 'https://github.com/Graphify-Labs/graphify', ctaLabelKey: 'publicEvidence', publish: true,
    evidence: { kind: 'public-repository', href: 'https://github.com/Graphify-Labs/graphify', reviewedOn: '2026-11-24', scope: 'description-only' },
  },
];
