import { commands } from './bindings'
import { Channel, convertFileSrc } from '@tauri-apps/api/core'
import type {
  CloseChoiceDto,
  ComfyEventDto,
  LaunchOptionsDto,
  UpdateInput,
  SetupActionDto,
  SetupEventDto,
  PythonSourceDto,
  PipOperationDto,
  PipApplyInput,
  DownloadEventDto,
  EnqueueItemInput,
  ExportInput,
  ImportPackInput,
  ResolveInput,
  ConflictChoiceDto,
  IdentifyInput,
  ModelEventDto,
  ModelLocationDto,
  MoveInput,
  RenameInput,
  RegistrySearchInput,
  PrepareRegistryInput,
  PrepareGitInput,
  PrepareUpdateInput,
  PrepareRetryInput,
  CommitInput,
  SetEnabledInput,
  NodeEventDto,
  NodeGitStatusDto,
  UpdateProgressDto,
  GalleryEntryRefDto,
  GalleryLocationDto,
  GalleryRenameInput,
  GalleryTransferInput,
} from './bindings'
export type * from './bindings'
import type {
  AppInfoDto,
  CloseBehaviorDto,
  CredentialBackendDto,
  CredentialKindDto,
  CredentialStatusDto,
  HfEngineDto,
  IpcError,
  SettingsDto,
  ThemeDto,
  UpdateSettingsDto,
} from './bindings'

export type {
  AppInfoDto,
  CloseBehaviorDto,
  CredentialBackendDto,
  CredentialKindDto,
  CredentialStatusDto,
  HfEngineDto,
  SettingsDto,
  ThemeDto,
  UpdateSettingsDto,
}

/** The only error type that leaves the gateway. */
export class IpcClientError extends Error {
  readonly code: string

  constructor(code: string, message: string) {
    super(message)
    this.name = 'IpcClientError'
    this.code = code
  }
}

type IpcResult<T> =
  | { status: 'ok'; data: T }
  | { status: 'error'; error: IpcError }

async function unwrap<T>(call: () => Promise<IpcResult<T>>): Promise<T> {
  let result: IpcResult<T>
  try {
    result = await call()
  } catch {
    // Transport failures can carry arbitrary text, so none of it is surfaced.
    throw new IpcClientError(
      'transport',
      'Could not reach the application backend.',
    )
  }
  if (result.status === 'error') {
    throw new IpcClientError(result.error.code, result.error.message)
  }
  return result.data
}

export const gateway = {
  appInfo: (): Promise<AppInfoDto> => unwrap(() => commands.appInfo()),
  settingsGet: (): Promise<SettingsDto> => unwrap(() => commands.settingsGet()),
  settingsSet: (input: UpdateSettingsDto): Promise<SettingsDto> =>
    unwrap(() => commands.settingsSet(input)),
  /** Takes the secret straight from the caller. It is never cached, logged or stored. */
  secretSet: (kind: CredentialKindDto, value: string): Promise<null> =>
    unwrap(() => commands.secretSet({ kind, value })),
  secretClear: (kind: CredentialKindDto): Promise<null> =>
    unwrap(() => commands.secretClear(kind)),
  secretStatus: (kind: CredentialKindDto): Promise<CredentialStatusDto> =>
    unwrap(() => commands.secretStatus(kind)),
  /** Tests the saved token against its provider; the result carries no profile data. */
  secretTest: (kind: CredentialKindDto) =>
    unwrap(() => commands.secretTest(kind)),
  setupStatus: () => unwrap(() => commands.setupStatus()),
  setupDetectHardware: (
    root: string | null = null,
    envPath: string | null = null,
  ) => unwrap(() => commands.setupDetectHardware(root, envPath)),
  setupListProfiles: (
    root: string | null = null,
    envPath: string | null = null,
  ) => unwrap(() => commands.setupListProfiles(root, envPath)),
  setupListCuratedNodes: () => unwrap(() => commands.setupListCuratedNodes()),
  setupValidatePython: (source: PythonSourceDto) =>
    unwrap(() => commands.setupValidatePython(source)),
  setupSelectRoot: (path: string, draft: string | null) =>
    unwrap(() => commands.setupSelectRoot(path, draft)),
  setupOpenInstall: (path: string) =>
    unwrap(() => commands.setupOpenInstall(path)),
  setupNewInstall: (path: string) =>
    unwrap(() => commands.setupNewInstall(path)),
  setupRestart: () => unwrap(() => commands.setupRestart()),
  setupRun: (action: SetupActionDto, onEvent: (event: SetupEventDto) => void) =>
    unwrap(() => {
      const channel = new Channel<SetupEventDto>()
      channel.onmessage = onEvent
      return commands.setupRun(action, channel)
    }),
  setupCancel: (runId: string) => unwrap(() => commands.setupCancel(runId)),
  envInfo: () => unwrap(() => commands.envInfo()),
  envPackages: () => unwrap(() => commands.envPackages()),
  envPipPreview: (operation: PipOperationDto) =>
    unwrap(() => commands.envPipPreview(operation)),
  envPipApply: (input: PipApplyInput) =>
    unwrap(() => commands.envPipApply(input)),
  envDelete: (envId: string) => unwrap(() => commands.envDelete(envId)),
  comfyStatus: () => unwrap(() => commands.comfyStatus()),
  comfyStart: () => unwrap(() => commands.comfyStart()),
  comfyStop: () => unwrap(() => commands.comfyStop()),
  comfyRestart: () => unwrap(() => commands.comfyRestart()),
  /** Resolves to the subscription id. The first event is an authoritative snapshot. */
  comfySubscribe: (
    afterCursor: number | null,
    onEvent: (event: ComfyEventDto) => void,
  ) =>
    unwrap(() => {
      const channel = new Channel<ComfyEventDto>()
      channel.onmessage = onEvent
      return commands.comfySubscribe(afterCursor, channel)
    }),
  comfyUnsubscribe: (subscriptionId: number) =>
    unwrap(() => commands.comfyUnsubscribe(subscriptionId)),
  comfyGetLaunchOptions: () => unwrap(() => commands.comfyGetLaunchOptions()),
  comfySetLaunchOptions: (options: LaunchOptionsDto) =>
    unwrap(() => commands.comfySetLaunchOptions(options)),
  comfyListVersions: () => unwrap(() => commands.comfyListVersions()),
  comfyUpdate: (input: UpdateInput) =>
    unwrap(() => commands.comfyUpdate(input)),
  comfyOpenWindow: () => unwrap(() => commands.comfyOpenWindow()),
  comfyOpenBrowser: () => unwrap(() => commands.comfyOpenBrowser()),
  comfyReadExtraModelPaths: () =>
    unwrap(() => commands.comfyReadExtraModelPaths()),
  comfyGenerateExtraModelPaths: (basePath: string) =>
    unwrap(() => commands.comfyGenerateExtraModelPaths(basePath)),
  comfyRemoveExtraModelPaths: () =>
    unwrap(() => commands.comfyRemoveExtraModelPaths()),
  appCloseDecide: (requestId: string, choice: CloseChoiceDto) =>
    unwrap(() => commands.appCloseDecide(requestId, choice)),
  dlResolve: (input: ResolveInput) => unwrap(() => commands.dlResolve(input)),
  dlEnqueue: (items: EnqueueItemInput[]) =>
    unwrap(() => commands.dlEnqueue(items)),
  dlPause: (jobId: string) => unwrap(() => commands.dlPause(jobId)),
  dlCancel: (jobId: string) => unwrap(() => commands.dlCancel(jobId)),
  dlRetry: (jobId: string) => unwrap(() => commands.dlRetry(jobId)),
  dlResume: (
    jobId: string,
    decision: { decisionId: string; choice: ConflictChoiceDto } | null = null,
  ) => unwrap(() => commands.dlResume(jobId, decision)),
  /** Resolves to the subscription id. The first event is an authoritative snapshot. */
  dlSubscribe: (
    afterCursor: { epoch: string; sequence: number | null } | null,
    onEvent: (event: DownloadEventDto) => void,
  ) =>
    unwrap(() => {
      const channel = new Channel<DownloadEventDto>()
      channel.onmessage = onEvent
      return commands.dlSubscribe(afterCursor, channel)
    }),
  dlUnsubscribe: (subscriptionId: number) =>
    unwrap(() => commands.dlUnsubscribe(subscriptionId)),
  dlImportPack: (input: ImportPackInput) =>
    unwrap(() => commands.dlImportPack(input)),
  dlExport: (input: ExportInput) => unwrap(() => commands.dlExport(input)),
  modelList: () => unwrap(() => commands.modelList()),
  /** Finishes interrupted operations, scans every root and resolves with the new list. */
  modelRescan: () => unwrap(() => commands.modelRescan()),
  modelDetails: (location: ModelLocationDto) =>
    unwrap(() => commands.modelDetails(location)),
  modelRename: (input: RenameInput) =>
    unwrap(() => commands.modelRename(input)),
  modelMove: (input: MoveInput) => unwrap(() => commands.modelMove(input)),
  modelDeletePlan: (locations: ModelLocationDto[]) =>
    unwrap(() => commands.modelDeletePlan(locations)),
  modelDelete: (planId: string) => unwrap(() => commands.modelDelete(planId)),
  /** Only runs a permanent-delete plan produced after a trash failure. */
  modelDeletePermanently: (planId: string) =>
    unwrap(() => commands.modelDeletePermanently(planId)),
  /** Shows the model in the system file manager, or the models folder when `null`. */
  modelReveal: (location: ModelLocationDto | null): Promise<null> =>
    unwrap(() => commands.modelReveal(location)),
  modelIdentify: (input: IdentifyInput) =>
    unwrap(() => commands.modelIdentify(input)),
  modelOperationStatus: () => unwrap(() => commands.modelOperationStatus()),
  /** Resolves to the subscription id. The first event is the current status. */
  modelSubscribe: (onEvent: (event: ModelEventDto) => void) =>
    unwrap(() => {
      const channel = new Channel<ModelEventDto>()
      channel.onmessage = onEvent
      return commands.modelSubscribe(channel)
    }),
  modelUnsubscribe: (subscriptionId: number) =>
    unwrap(() => commands.modelUnsubscribe(subscriptionId)),
  nodeRegistrySearch: (input: RegistrySearchInput) =>
    unwrap(() => commands.nodeRegistrySearch(input)),
  nodeList: () => unwrap(() => commands.nodeList()),
  nodeOpenRepository: (repository: string): Promise<null> =>
    unwrap(() => commands.nodeOpenRepository(repository)),
  /** Shows the node's folder in the system file manager, or the custom nodes folder when `null`. */
  nodeReveal: (id: string | null): Promise<null> =>
    unwrap(() => commands.nodeReveal(id)),
  nodeGitStatus: (nodeId: string): Promise<NodeGitStatusDto> =>
    unwrap(() => commands.nodeGitStatus({ nodeId })),
  nodeRescan: () => unwrap(() => commands.nodeRescan()),
  nodeResolveOperation: (operationId: string) => unwrap(() => commands.nodeResolveOperation(operationId)),
  nodePrepareRegistry: (input: PrepareRegistryInput) =>
    unwrap(() => commands.nodePrepareRegistry(input)),
  nodePrepareGit: (input: PrepareGitInput) =>
    unwrap(() => commands.nodePrepareGit(input)),
  nodePrepareUpdate: (input: PrepareUpdateInput) =>
    unwrap(() => commands.nodePrepareUpdate(input)),
  nodePrepareRetryDeps: (input: PrepareRetryInput) =>
    unwrap(() => commands.nodePrepareRetryDeps(input)),
  nodeCommit: (input: CommitInput) => unwrap(() => commands.nodeCommit(input)),
  nodeSetEnabled: (input: SetEnabledInput) =>
    unwrap(() => commands.nodeSetEnabled(input)),
  nodeUninstall: (nodeId: string) =>
    unwrap(() => commands.nodeUninstall({ nodeId })),
  nodeUpdateAll: (items: CommitInput[]) =>
    unwrap(() => commands.nodeUpdateAll({ items })),
  nodeCancel: (key: string) => unwrap(() => commands.nodeCancel(key)),
  nodeOperationStatus: (operationId: string) =>
    unwrap(() => commands.nodeOperationStatus(operationId)),
  nodeSubscribe: (onEvent: (event: NodeEventDto) => void) =>
    unwrap(() => {
      const channel = new Channel<NodeEventDto>()
      channel.onmessage = onEvent
      return commands.nodeSubscribe(channel)
    }),
  nodeUnsubscribe: (subscriptionId: number) =>
    unwrap(() => commands.nodeUnsubscribe(subscriptionId)),
  appUpdateStatus: () => unwrap(() => commands.appUpdateStatus()),
  /** Resolves to the newer release for the user to review, or null. */
  appUpdateCheck: () => unwrap(() => commands.appUpdateCheck()),
  /** Installs exactly the reviewed release named by its `updateId`; the app then restarts. */
  appUpdateInstall: (
    updateId: string,
    onEvent: (event: UpdateProgressDto) => void,
  ) =>
    unwrap(() => {
      const channel = new Channel<UpdateProgressDto>()
      channel.onmessage = onEvent
      return commands.appUpdateInstall(updateId, channel)
    }),
  /** Copies the redacted report; `copied` is false when the clipboard was unavailable. */
  appCopyDiagnostics: () => unwrap(() => commands.appCopyDiagnostics()),
  galleryList: (directory: GalleryLocationDto) =>
    unwrap(() => commands.galleryList(directory)),
  galleryRename: (input: GalleryRenameInput) =>
    unwrap(() => commands.galleryRename(input)),
  galleryMove: (input: GalleryTransferInput) =>
    unwrap(() => commands.galleryMove(input)),
  galleryCopy: (input: GalleryTransferInput) =>
    unwrap(() => commands.galleryCopy(input)),
  galleryDeletePlan: (items: GalleryEntryRefDto[]) =>
    unwrap(() => commands.galleryDeletePlan(items)),
  galleryDelete: (planId: string) =>
    unwrap(() => commands.galleryDelete(planId)),
  galleryReveal: (location: GalleryLocationDto): Promise<null> =>
    unwrap(() => commands.galleryReveal(location)),
  /** `<base>` of the `gallery` protocol, e.g. for validating loader URLs. */
  galleryPreviewBase: (): string => convertFileSrc('', 'gallery'),
  /** Hierarchical URL on the app's `gallery` protocol, so glTF-relative resources resolve inside the same folder. `revision` busts the webview cache. */
  galleryPreviewUrl: (location: GalleryLocationDto, revision: string): string =>
    `${convertFileSrc('', 'gallery')}${location.area}/${location.path
      .split('/')
      .filter(Boolean)
      .map(encodeURIComponent)
      .join('/')}?v=${encodeURIComponent(revision)}`,
  pickFolder: (title: string) => unwrap(() => commands.pickFolder(title)),
  pickFile: (title: string, extensions: string[] = []) =>
    unwrap(() => commands.pickFile(title, extensions)),
}
