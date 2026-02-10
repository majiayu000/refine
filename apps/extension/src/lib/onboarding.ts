export type OnboardingTaskKey = 'extracted' | 'searched' | 'reused'

export interface OnboardingTaskState {
  extracted: boolean
  searched: boolean
  reused: boolean
  updatedAt: number
}

const ONBOARDING_KEY = 'onboardingTasks'

const DEFAULT_ONBOARDING_STATE: OnboardingTaskState = {
  extracted: false,
  searched: false,
  reused: false,
  updatedAt: 0,
}

function normalizeOnboardingState(raw: unknown): OnboardingTaskState {
  if (!raw || typeof raw !== 'object') return { ...DEFAULT_ONBOARDING_STATE }
  const candidate = raw as Partial<OnboardingTaskState>
  return {
    extracted: candidate.extracted === true,
    searched: candidate.searched === true,
    reused: candidate.reused === true,
    updatedAt: typeof candidate.updatedAt === 'number' ? candidate.updatedAt : 0,
  }
}

export async function readOnboardingTaskState(): Promise<OnboardingTaskState> {
  try {
    const stored = await chrome.storage.local.get([ONBOARDING_KEY])
    return normalizeOnboardingState(stored[ONBOARDING_KEY])
  } catch {
    return { ...DEFAULT_ONBOARDING_STATE }
  }
}

export async function markOnboardingTask(task: OnboardingTaskKey): Promise<void> {
  try {
    const current = await readOnboardingTaskState()
    if (current[task]) return
    const next: OnboardingTaskState = {
      ...current,
      [task]: true,
      updatedAt: Date.now(),
    }
    await chrome.storage.local.set({
      [ONBOARDING_KEY]: next,
    })
  } catch {
    // ignore storage failures
  }
}
