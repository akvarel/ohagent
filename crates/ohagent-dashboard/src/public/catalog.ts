import type { Copy } from './locale';

export interface ProductEvidence {
  kind: 'repository' | 'public-site' | 'public-repository';
  href: string;
  reviewedOn: '2026-11-24';
  scope: 'description-only';
}
export interface ProductEntry {
  id: 'ohagent' | 'bugzero' | 'graphify';
  displayName: string;
  kind: 'runtime' | 'product' | 'technology';
  categoryKey: keyof Copy;
  summaryKey: keyof Copy;
  targetUserKey: keyof Copy;
  verifiedStatus: 'pilot-code-only' | 'availability-unverified' | 'public-source';
  statusKey: keyof Copy;
  relationship: 'orangehat-runtime' | 'public-product-reference' | 'third-party-tooling';
  primaryUrl: string;
  ctaLabelKey: keyof Copy;
  publish: boolean;
  evidence: ProductEvidence;
}
// Review verifies the cited description only. Status does not imply production
// availability, ownership, affiliation, pricing or a working integration.
export const productCatalog: readonly ProductEntry[] = [
  {
    id: 'ohagent', displayName: 'ohAgent', kind: 'runtime', categoryKey: 'runtime', summaryKey: 'ohDescription', targetUserKey: 'ohUseCase',
    verifiedStatus: 'pilot-code-only', statusKey: 'pilotCode', relationship: 'orangehat-runtime', primaryUrl: '/agent-as-a-service', ctaLabelKey: 'learnMore', publish: true,
    evidence: { kind: 'repository', href: 'https://github.com/akvarel/ohagent', reviewedOn: '2026-11-24', scope: 'description-only' },
  },
  {
    id: 'bugzero', displayName: 'BugZero', kind: 'product', categoryKey: 'publicProduct', summaryKey: 'bugDescription', targetUserKey: 'bugUseCase',
    verifiedStatus: 'availability-unverified', statusKey: 'availabilityUnverified', relationship: 'public-product-reference', primaryUrl: 'https://bugzero.lv/', ctaLabelKey: 'publicEvidence', publish: true,
    evidence: { kind: 'public-site', href: 'https://bugzero.lv/', reviewedOn: '2026-11-24', scope: 'description-only' },
  },
  {
    id: 'graphify', displayName: 'Graphify', kind: 'technology', categoryKey: 'openSource', summaryKey: 'graphDescription', targetUserKey: 'graphUseCase',
    verifiedStatus: 'public-source', statusKey: 'sourceAvailable', relationship: 'third-party-tooling', primaryUrl: 'https://github.com/Graphify-Labs/graphify', ctaLabelKey: 'publicEvidence', publish: true,
    evidence: { kind: 'public-repository', href: 'https://github.com/Graphify-Labs/graphify', reviewedOn: '2026-11-24', scope: 'description-only' },
  },
];
