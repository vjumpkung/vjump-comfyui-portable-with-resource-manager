/** ComfyUI `models/` subfolders (REQUIREMENTS 4.6). Any other valid folder name is accepted. */
export const MODEL_CATEGORIES = [
  'checkpoints',
  'diffusion_models',
  'text_encoders',
  'vae',
  'loras',
  'controlnet',
  'clip_vision',
  'embeddings',
  'upscale_models',
  'latent_upscale_models',
  'style_models',
  'hypernetworks',
  'photomaker',
  'gligen',
  'audio_encoders',
  'model_patches',
  'vae_approx',
  'diffusers',
] as const

export type ProviderGuess = 'civitai' | 'huggingface' | 'google_drive' | 'direct'

/** A quick hint for the provider chip while typing; the backend decides authoritatively. */
export function guessProvider(url: string): ProviderGuess | null {
  let host: string
  try {
    host = new URL(url.trim()).hostname.toLowerCase()
  } catch {
    return null
  }
  if (['civitai.com', 'civitai.green', 'civitai.red'].includes(host))
    return 'civitai'
  if (host === 'huggingface.co') return 'huggingface'
  if (
    host === 'drive.google.com' ||
    host === 'docs.google.com' ||
    host === 'drive.usercontent.google.com'
  )
    return 'google_drive'
  return 'direct'
}

export const PROVIDER_LABELS = {
  civitai: 'CivitAI',
  huggingface: 'Hugging Face',
  google_drive: 'Google Drive',
  direct: 'downloads.directLink',
} as const satisfies Record<ProviderGuess, string>
