import type { CopyKey } from './i18n'

export class ClientError extends Error {
  constructor(public key: CopyKey, public code = '', public status = 0) { super(key) }
}
export function asClientError(error: unknown): ClientError {
  if (error instanceof ClientError) return error
  if (error instanceof DOMException && error.name === 'TimeoutError') return new ClientError('timeout')
  return new ClientError('network')
}
export async function responseError(response: Response): Promise<ClientError> {
  let code = ''
  try { const body = await response.json(); const candidate = body.code ?? body.error?.code ?? body.error; if (typeof candidate === 'string' && /^[a-zA-Z0-9_.-]{1,80}$/.test(candidate)) code = candidate } catch {}
  const key: CopyKey = response.status === 401 ? 'unauthorized' : response.status === 403 ? 'forbidden' : response.status === 404 ? 'notFound' : response.status === 409 ? 'conflict' : response.status === 422 || response.status === 400 ? 'invalidInput' : response.status === 503 ? 'unavailable' : 'error'
  return new ClientError(key, code, response.status)
}
