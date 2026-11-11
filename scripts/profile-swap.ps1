# Developer-only, Windows-only swap of local app state between the default
# profile and a tutorial-recording profile (see README, "Tutorial recording
# profile"). Run through swap-to-tutorial-profile.cmd / restore-default-profile.cmd.
#
# Windows PowerShell 5.1 compatible and ASCII only. Folders are renamed in place;
# stored tokens are copied inside Windows Credential Manager and the source is
# deleted only after the copy is verified. Secrets are handled only inside the
# C# helper below and never become PowerShell strings or output.
#
# Exit codes: 0 done or nothing to do, 1 refused/cancelled/failed, 2 bad parameters.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('tutorial', 'default')]
    [string]$To,

    [switch]$NoPause,

    # Test-only: redirect every path and credential name into a sandbox. Both must be given together.
    [Parameter(DontShow)]
    [string]$SandboxRoot,

    [Parameter(DontShow)]
    [string]$CredentialService
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

function Say([string]$Text) {
    [Console]::Out.WriteLine($Text)
}

function Stop-Usage([string]$Message) {
    [Console]::Error.WriteLine($Message)
    exit 2
}

function Stop-Refused([string]$Message) {
    throw (New-Object System.InvalidOperationException($Message))
}

function Stop-Cancelled {
    throw (New-Object System.OperationCanceledException('Cancelled. Nothing changed.'))
}

function Get-NormalPath([string]$Path) {
    return [IO.Path]::GetFullPath($Path).TrimEnd('\')
}

# ---------------------------------------------------------------- path resolution

$sandboxGiven = $PSBoundParameters.ContainsKey('SandboxRoot')
$serviceGiven = $PSBoundParameters.ContainsKey('CredentialService')
if ($sandboxGiven -ne $serviceGiven) {
    Stop-Usage 'Usage error: -SandboxRoot and -CredentialService must be given together.'
}

$realAppData = Get-NormalPath ([Environment]::GetFolderPath('ApplicationData'))
$realLocalData = Get-NormalPath ([Environment]::GetFolderPath('LocalApplicationData'))
$realService = 'vjumpkung-comfyui'

if ($sandboxGiven) {
    if ($SandboxRoot -notmatch '^[A-Za-z]:[\\/]') {
        Stop-Usage 'Usage error: -SandboxRoot must be a fully qualified absolute path.'
    }
    if ($CredentialService -notmatch '^[A-Za-z0-9-]{1,64}\z') {
        Stop-Usage 'Usage error: -CredentialService may only contain letters, digits and hyphens (1 to 64).'
    }
    if ($CredentialService -ieq $realService) {
        Stop-Usage 'Usage error: -CredentialService must not be the real service name.'
    }
    $appData = Get-NormalPath ([IO.Path]::Combine($SandboxRoot, 'Roaming'))
    $localData = Get-NormalPath ([IO.Path]::Combine($SandboxRoot, 'Local'))
    $service = $CredentialService
    foreach ($pair in @(
            @(($appData + '\vjumpkung-comfyui'), ($realAppData + '\vjumpkung-comfyui')),
            @(($localData + '\vjumpkung-comfyui'), ($realLocalData + '\vjumpkung-comfyui')),
            @(($localData + '\dev.vjump.comfyui-desktop'), ($realLocalData + '\dev.vjump.comfyui-desktop')))) {
        if ($pair[0] -ieq $pair[1]) {
            Stop-Usage 'Usage error: the sandbox resolves to the real application folders.'
        }
    }
    [void][IO.Directory]::CreateDirectory($appData)
    [void][IO.Directory]::CreateDirectory($localData)
} else {
    $appData = $realAppData
    $localData = $realLocalData
    $service = $realService
}

$script:AppData = $appData
$script:LocalData = $localData
$script:StatePath = $localData + '\vjumpkung-comfyui.profile-swap-state'
$script:Items = @(
    [pscustomobject]@{ Id = 'bootstrap'; Live = ($appData + '\vjumpkung-comfyui'); Kind = 'dir' },
    [pscustomobject]@{ Id = 'root'; Live = ($localData + '\vjumpkung-comfyui'); Kind = 'dir' },
    [pscustomobject]@{ Id = 'webview'; Live = ($localData + '\dev.vjump.comfyui-desktop'); Kind = 'dir' },
    [pscustomobject]@{ Id = 'civitai'; Live = ('civitai.' + $service); Kind = 'credential' },
    [pscustomobject]@{ Id = 'huggingface'; Live = ('huggingface.' + $service); Kind = 'credential' }
)

# ---------------------------------------------------------------- credential helper

$helperSource = @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

namespace VjProfileSwap
{
    public static class Credentials
    {
        private const uint TypeGeneric = 1;
        private const int ErrorNotFound = 1168;

        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        private struct CREDENTIALW
        {
            public uint Flags;
            public uint Type;
            public IntPtr TargetName;
            public IntPtr Comment;
            public System.Runtime.InteropServices.ComTypes.FILETIME LastWritten;
            public uint CredentialBlobSize;
            public IntPtr CredentialBlob;
            public uint Persist;
            public uint AttributeCount;
            public IntPtr Attributes;
            public IntPtr TargetAlias;
            public IntPtr UserName;
        }

        [DllImport("advapi32.dll", EntryPoint = "CredReadW", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern bool CredRead(string target, uint type, uint flags, out IntPtr credential);

        [DllImport("advapi32.dll", EntryPoint = "CredWriteW", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern bool CredWrite(ref CREDENTIALW credential, uint flags);

        [DllImport("advapi32.dll", EntryPoint = "CredDeleteW", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern bool CredDelete(string target, uint type, uint flags);

        [DllImport("advapi32.dll", EntryPoint = "CredFree", SetLastError = true)]
        private static extern void CredFree(IntPtr buffer);

        private sealed class Snapshot
        {
            public uint Flags;
            public uint Type;
            public uint Persist;
            public uint AttributeCount;
            public string UserName;
            public string Comment;
            public string TargetAlias;
            public byte[] Blob;

            public void Clear()
            {
                if (Blob != null)
                {
                    Array.Clear(Blob, 0, Blob.Length);
                }
            }
        }

        // Returns null when the credential does not exist.
        private static Snapshot Read(string target)
        {
            IntPtr buffer;
            if (!CredRead(target, TypeGeneric, 0, out buffer))
            {
                int error = Marshal.GetLastWin32Error();
                if (error == ErrorNotFound)
                {
                    return null;
                }
                throw new Win32Exception(error);
            }
            try
            {
                CREDENTIALW c = (CREDENTIALW)Marshal.PtrToStructure(buffer, typeof(CREDENTIALW));
                Snapshot s = new Snapshot();
                s.Flags = c.Flags;
                s.Type = c.Type;
                s.Persist = c.Persist;
                s.AttributeCount = c.AttributeCount;
                s.UserName = Marshal.PtrToStringUni(c.UserName);
                s.Comment = Marshal.PtrToStringUni(c.Comment);
                s.TargetAlias = Marshal.PtrToStringUni(c.TargetAlias);
                int size = (int)c.CredentialBlobSize;
                s.Blob = new byte[size];
                if (size > 0 && c.CredentialBlob != IntPtr.Zero)
                {
                    Marshal.Copy(c.CredentialBlob, s.Blob, 0, size);
                    Marshal.Copy(new byte[size], 0, c.CredentialBlob, size);
                }
                return s;
            }
            finally
            {
                CredFree(buffer);
            }
        }

        private static bool SameContent(Snapshot a, Snapshot b)
        {
            if (a.Persist != b.Persist)
            {
                return false;
            }
            if (!string.Equals(a.UserName ?? "", b.UserName ?? "", StringComparison.Ordinal))
            {
                return false;
            }
            if (a.Blob.Length != b.Blob.Length)
            {
                return false;
            }
            int difference = 0;
            for (int i = 0; i < a.Blob.Length; i++)
            {
                difference |= a.Blob[i] ^ b.Blob[i];
            }
            return difference == 0;
        }

        // 0 = absent, 1 = valid generic credential, -2 = shape this tool will not move.
        public static int Probe(string target)
        {
            Snapshot s = Read(target);
            if (s == null)
            {
                return 0;
            }
            try
            {
                if (s.Type != TypeGeneric || s.Flags != 0 || s.AttributeCount != 0)
                {
                    return -2;
                }
                return 1;
            }
            finally
            {
                s.Clear();
            }
        }

        // True when both exist and have identical blob bytes, user name and persistence.
        public static bool Same(string a, string b)
        {
            Snapshot sa = Read(a);
            Snapshot sb = null;
            try
            {
                if (sa == null)
                {
                    return false;
                }
                sb = Read(b);
                if (sb == null)
                {
                    return false;
                }
                return SameContent(sa, sb);
            }
            finally
            {
                if (sa != null)
                {
                    sa.Clear();
                }
                if (sb != null)
                {
                    sb.Clear();
                }
            }
        }

        // Copies blob, user name, comment, alias and persistence verbatim; only the target name changes.
        public static void Copy(string source, string destination)
        {
            Snapshot s = Read(source);
            if (s == null)
            {
                throw new InvalidOperationException("Stored credential not found: " + source);
            }
            Snapshot back = null;
            IntPtr target = IntPtr.Zero;
            IntPtr user = IntPtr.Zero;
            IntPtr comment = IntPtr.Zero;
            IntPtr alias = IntPtr.Zero;
            IntPtr blob = IntPtr.Zero;
            int size = s.Blob.Length;
            try
            {
                target = Marshal.StringToCoTaskMemUni(destination);
                if (s.UserName != null)
                {
                    user = Marshal.StringToCoTaskMemUni(s.UserName);
                }
                if (s.Comment != null)
                {
                    comment = Marshal.StringToCoTaskMemUni(s.Comment);
                }
                if (s.TargetAlias != null)
                {
                    alias = Marshal.StringToCoTaskMemUni(s.TargetAlias);
                }
                if (size > 0)
                {
                    blob = Marshal.AllocCoTaskMem(size);
                    Marshal.Copy(s.Blob, 0, blob, size);
                }
                CREDENTIALW c = new CREDENTIALW();
                c.Flags = 0;
                c.Type = TypeGeneric;
                c.TargetName = target;
                c.Comment = comment;
                c.CredentialBlobSize = (uint)size;
                c.CredentialBlob = blob;
                c.Persist = s.Persist;
                c.AttributeCount = 0;
                c.Attributes = IntPtr.Zero;
                c.TargetAlias = alias;
                c.UserName = user;
                if (!CredWrite(ref c, 0))
                {
                    throw new Win32Exception(Marshal.GetLastWin32Error());
                }
                back = Read(destination);
                if (back == null || !SameContent(s, back))
                {
                    throw new InvalidOperationException("Stored credential copy did not verify: " + destination);
                }
            }
            finally
            {
                if (blob != IntPtr.Zero)
                {
                    Marshal.Copy(new byte[size], 0, blob, size);
                    Marshal.FreeCoTaskMem(blob);
                }
                if (target != IntPtr.Zero) { Marshal.FreeCoTaskMem(target); }
                if (user != IntPtr.Zero) { Marshal.FreeCoTaskMem(user); }
                if (comment != IntPtr.Zero) { Marshal.FreeCoTaskMem(comment); }
                if (alias != IntPtr.Zero) { Marshal.FreeCoTaskMem(alias); }
                s.Clear();
                if (back != null)
                {
                    back.Clear();
                }
            }
        }

        public static void Delete(string target)
        {
            if (!CredDelete(target, TypeGeneric, 0))
            {
                int error = Marshal.GetLastWin32Error();
                if (error != ErrorNotFound)
                {
                    throw new Win32Exception(error);
                }
            }
        }
    }
}
'@

# ---------------------------------------------------------------- generic helpers

function Get-ItemPath($Item, [string]$Where) {
    if ($Where -eq 'live') {
        return $Item.Live
    }
    return ($Item.Live + '.profile-' + $Where)
}

function Test-Inside([string]$Path, [string]$Parent) {
    $candidate = $Path.TrimEnd('\') + '\'
    $root = $Parent.TrimEnd('\') + '\'
    return $candidate.StartsWith($root, [StringComparison]::OrdinalIgnoreCase)
}

function Test-Present($Item, [string]$Path) {
    if ($Item.Kind -eq 'dir') {
        return ([IO.Directory]::Exists($Path) -or [IO.File]::Exists($Path))
    }
    return ([VjProfileSwap.Credentials]::Probe($Path) -ne 0)
}

function Get-PresentStash([string]$Name) {
    $found = New-Object System.Collections.ArrayList
    foreach ($item in $script:Items) {
        $path = Get-ItemPath $item $Name
        if (Test-Present $item $path) {
            [void]$found.Add($path)
        }
    }
    return $found.ToArray()
}

function Read-Answer([string]$Prompt) {
    [Console]::Out.Write($Prompt)
    [Console]::Out.Flush()
    return [Console]::In.ReadLine()
}

function Test-Answer($Answer, [string[]]$Accepted) {
    if ($null -eq $Answer) {
        return $false
    }
    return ($Accepted -contains $Answer.Trim().ToLowerInvariant())
}

function Invoke-Pause {
    if ($NoPause) {
        return
    }
    if ([Console]::IsInputRedirected) {
        return
    }
    [Console]::Out.Write('Press Enter to close')
    [Console]::Out.Flush()
    [void][Console]::In.ReadLine()
}

# ---------------------------------------------------------------- swap state file

function Get-SwapState {
    if (-not [IO.File]::Exists($script:StatePath)) {
        return $null
    }
    return [IO.File]::ReadAllText($script:StatePath).Trim()
}

function Set-SwapState([string]$Value) {
    $temporary = $script:StatePath + '.tmp'
    [IO.File]::WriteAllText($temporary, $Value, [Text.Encoding]::ASCII)
    if ([IO.File]::Exists($script:StatePath)) {
        [IO.File]::Replace($temporary, $script:StatePath, [NullString]::Value)
    } else {
        [IO.File]::Move($temporary, $script:StatePath)
    }
}

function Remove-SwapState {
    [IO.File]::Delete($script:StatePath)
}

# ---------------------------------------------------------------- moves

function Move-Directory([string]$Src, [string]$Dst, [bool]$CheckOnly) {
    $srcIsDirectory = [IO.Directory]::Exists($Src)
    if (-not $srcIsDirectory -and -not [IO.File]::Exists($Src)) {
        return $false
    }
    if (-not $srcIsDirectory) {
        Stop-Refused ('Expected a folder but found a file: ' + $Src + '. Nothing was changed.')
    }
    if ([IO.File]::Exists($Dst)) {
        Stop-Refused ('Expected a folder but found a file: ' + $Dst + '. Nothing was changed.')
    }
    if ([IO.Directory]::Exists($Dst)) {
        Stop-Refused ('Both ' + $Src + ' and ' + $Dst + ' exist; refusing to overwrite. Rename or remove one of them, then run this script again.')
    }
    if ($CheckOnly) {
        return $true
    }
    Say ('Moving ' + $Src + '  ->  ' + $Dst)
    $lastError = ''
    for ($attempt = 1; $attempt -le 5; $attempt++) {
        try {
            [IO.Directory]::Move($Src, $Dst)
            return $true
        } catch [System.IO.IOException], [System.UnauthorizedAccessException] {
            $lastError = $_.Exception.Message
            if ($attempt -lt 5) {
                Start-Sleep -Seconds 1
            }
        }
    }
    Stop-Refused ('Could not move ' + $Src + ' (' + $lastError + ').' + [Environment]::NewLine + 'Nothing is lost. Close Explorer, terminal or editor windows inside ' + $Src + ' and run this script again, or run restore-default-profile.cmd to go back.')
}

function Move-Credential([string]$Src, [string]$Dst, [bool]$CheckOnly) {
    $srcState = [VjProfileSwap.Credentials]::Probe($Src)
    if ($srcState -eq 0) {
        return $false
    }
    $dstState = [VjProfileSwap.Credentials]::Probe($Dst)
    if ($srcState -lt 0 -or $dstState -lt 0) {
        Stop-Refused ('Stored credential ' + $Src + ' or ' + $Dst + ' has an unsupported shape. Nothing was changed.')
    }
    if ($dstState -eq 1) {
        # CredWriteW silently overwrites, so an existing destination must be checked first.
        if (-not [VjProfileSwap.Credentials]::Same($Src, $Dst)) {
            Stop-Refused ('Both stored credentials ' + $Src + ' and ' + $Dst + ' exist and differ; refusing to overwrite. Remove one of them in Credential Manager, then run this script again.')
        }
        if ($CheckOnly) {
            return $true
        }
        Say ('Finishing an interrupted copy of stored token ' + $Src)
        [VjProfileSwap.Credentials]::Delete($Src)
    } else {
        if ($CheckOnly) {
            return $true
        }
        Say ('Moving stored token ' + $Src + '  ->  ' + $Dst)
        [VjProfileSwap.Credentials]::Copy($Src, $Dst)
        [VjProfileSwap.Credentials]::Delete($Src)
    }
    if ([VjProfileSwap.Credentials]::Probe($Src) -ne 0) {
        Stop-Refused ('Stored token ' + $Src + ' is still present after the move. Run this script again.')
    }
    return $true
}

function Move-ProfileItem($Item, [string]$From, [string]$Target, [bool]$CheckOnly) {
    $src = Get-ItemPath $Item $From
    $dst = Get-ItemPath $Item $Target
    if ($Item.Kind -eq 'dir') {
        return (Move-Directory $src $dst $CheckOnly)
    }
    return (Move-Credential $src $dst $CheckOnly)
}

# A phase moves all five items between two locations: live, default or tutorial.
function Invoke-Phase([string]$From, [string]$Target, [bool]$CheckOnly) {
    foreach ($item in $script:Items) {
        [void](Move-ProfileItem $item $From $Target $CheckOnly)
    }
}

function Show-Moves([string]$From, [string]$Target) {
    $any = $false
    foreach ($item in $script:Items) {
        $src = Get-ItemPath $item $From
        if (Test-Present $item $src) {
            $prefix = '  '
            if ($item.Kind -eq 'credential') {
                $prefix = '  stored token '
            }
            Say ($prefix + $src + '  ->  ' + (Get-ItemPath $item $Target))
            $any = $true
        }
    }
    if (-not $any) {
        Say '  (nothing to move)'
    }
}

function Assert-CredentialShapes {
    foreach ($item in $script:Items) {
        if ($item.Kind -ne 'credential') {
            continue
        }
        foreach ($where in @('live', 'default', 'tutorial')) {
            $target = Get-ItemPath $item $where
            if ([VjProfileSwap.Credentials]::Probe($target) -eq -2) {
                Stop-Refused ('Stored credential ' + $target + ' has an unsupported shape (not a plain generic credential). Nothing was changed.')
            }
        }
    }
}

# ---------------------------------------------------------------- bootstrap install roots

function Read-InstallRoot([string]$BootstrapFile) {
    if (-not [IO.File]::Exists($BootstrapFile)) {
        return $null
    }
    try {
        $text = [IO.File]::ReadAllText($BootstrapFile, [Text.Encoding]::UTF8)
        $document = ConvertFrom-Json -InputObject $text
    } catch {
        Say ('Warning: could not read ' + $BootstrapFile + '; ignoring its install_root.')
        return $null
    }
    if ($null -eq $document) {
        return $null
    }
    $property = $document.PSObject.Properties['install_root']
    if ($null -eq $property -or $property.Value -isnot [string] -or [string]::IsNullOrWhiteSpace($property.Value)) {
        return $null
    }
    $value = [string]$property.Value
    try {
        $full = [IO.Path]::GetFullPath($value)
    } catch {
        Say ('Warning: install_root in ' + $BootstrapFile + ' is not a usable path; ignoring it.')
        return $null
    }
    if ($full -notmatch '^([A-Za-z]:\\|\\\\[^\\]+\\[^\\]+)') {
        Say ('Warning: install_root in ' + $BootstrapFile + ' is not an absolute path; ignoring it.')
        return $null
    }
    if ($full.TrimEnd('\') -eq [IO.Path]::GetPathRoot($full).TrimEnd('\')) {
        Say ('Warning: install_root in ' + $BootstrapFile + ' is a drive root; ignoring it.')
        return $null
    }
    return $full.TrimEnd('\')
}

function Get-BootstrapFile([string]$Where) {
    return ((Get-ItemPath $script:Items[0] $Where) + '\bootstrap.json')
}

function Get-InstallRoots {
    $roots = New-Object System.Collections.ArrayList
    foreach ($where in @('live', 'default', 'tutorial')) {
        $root = Read-InstallRoot (Get-BootstrapFile $where)
        if ($null -ne $root) {
            [void]$roots.Add($root)
        }
    }
    return $roots.ToArray()
}

# ---------------------------------------------------------------- process check

function Get-GuardPaths {
    $paths = New-Object System.Collections.ArrayList
    foreach ($item in $script:Items) {
        if ($item.Kind -ne 'dir') {
            continue
        }
        foreach ($where in @('live', 'default', 'tutorial')) {
            [void]$paths.Add((Get-ItemPath $item $where))
        }
    }
    foreach ($root in @(Get-InstallRoots)) {
        [void]$paths.Add($root)
    }
    return $paths.ToArray()
}

function Assert-NoBlockers {
    $guards = @(Get-GuardPaths)
    $ignoreCase = [Text.RegularExpressions.RegexOptions]::IgnoreCase
    $patterns = New-Object System.Collections.ArrayList
    foreach ($guard in $guards) {
        [void]$patterns.Add((New-Object System.Text.RegularExpressions.Regex(([regex]::Escape($guard) + '(?=$|[\\"\s])'), $ignoreCase)))
    }
    try {
        $processes = @(Get-CimInstance -ClassName Win32_Process -ErrorAction Stop | Select-Object ProcessId, Name, ExecutablePath, CommandLine)
    } catch {
        Stop-Refused ('Could not list running programs (' + $_.Exception.Message + '). Nothing was changed.')
    }
    $blockers = New-Object System.Collections.ArrayList
    foreach ($proc in $processes) {
        if ($proc.ProcessId -eq $PID) {
            continue
        }
        $reason = $null
        if ($proc.Name -and ($proc.Name -ieq 'vjumpkung-comfyui-desktop.exe')) {
            $reason = 'the manager app is running'
        }
        if ($null -eq $reason -and $proc.ExecutablePath) {
            foreach ($guard in $guards) {
                if (Test-Inside $proc.ExecutablePath $guard) {
                    $reason = 'runs from ' + $guard
                    break
                }
            }
        }
        if ($null -eq $reason -and $proc.CommandLine) {
            for ($index = 0; $index -lt $patterns.Count; $index++) {
                if ($patterns[$index].IsMatch($proc.CommandLine)) {
                    $reason = 'command line uses ' + $guards[$index]
                    break
                }
            }
        }
        if ($null -ne $reason) {
            $location = $proc.ExecutablePath
            if (-not $location) {
                $location = '(path unavailable)'
            }
            [void]$blockers.Add(('  PID ' + $proc.ProcessId + '  ' + $proc.Name + '  ' + $location + '  [' + $reason + ']'))
        }
    }
    if ($blockers.Count -gt 0) {
        Say 'These programs are using the manager app or the folders that would be swapped:'
        foreach ($line in $blockers) {
            Say $line
        }
        Stop-Refused 'Close them and run this script again. Nothing was changed.'
    }
}

# ---------------------------------------------------------------- fresh-start cleanup

function Test-ReparsePoint([string]$Path) {
    $info = New-Object System.IO.DirectoryInfo($Path)
    return (($info.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)
}

function Remove-DiscardDirectory([string]$Path) {
    $parent = [IO.Path]::GetDirectoryName($Path)
    $leaf = [IO.Path]::GetFileName($Path)
    $parentOk = ($parent -ieq $script:AppData) -or ($parent -ieq $script:LocalData)
    $leafOk = $leaf -match '^(vjumpkung-comfyui|dev\.vjump\.comfyui-desktop)\.profile-discard-\d{14}\z'
    if (-not $parentOk -or -not $leafOk -or $Path.Contains('%')) {
        Say ('Warning: not deleting ' + $Path + ' because it does not look like a discard folder. Remove it by hand.')
        return
    }
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = [IO.Path]::Combine([Environment]::GetFolderPath('System'), 'cmd.exe')
    $info.Arguments = '/d /c rd /s /q "\\?\' + $Path + '"'
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $process = [Diagnostics.Process]::Start($info)
    $process.WaitForExit()
    $process.Dispose()
    if ([IO.Directory]::Exists($Path)) {
        Say ('Warning: could not fully delete ' + $Path + '. Remove it by hand when nothing uses it.')
    } else {
        Say ('Deleted ' + $Path)
    }
}

# ---------------------------------------------------------------- flows

function Invoke-ToTutorial {
    $state = Get-SwapState
    $tutorialPresent = @(Get-PresentStash 'tutorial')
    $defaultPresent = @(Get-PresentStash 'default')
    $resumeOnly = $false
    if ($null -eq $state) {
        if ($defaultPresent.Count -gt 0) {
            Say 'Found a saved default profile but no swap state:'
            foreach ($path in $defaultPresent) {
                Say ('  ' + $path)
            }
            Stop-Refused 'This is inconsistent, so nothing was changed. Inspect those items, then rename them back or remove them by hand.'
        }
    } elseif ($state -eq 'tutorial') {
        if ($tutorialPresent.Count -gt 0) {
            $resumeOnly = $true
        } else {
            Say 'Tutorial profile is already active.'
            return
        }
    } elseif ($state -eq 'stashing-tutorial' -or $state -eq 'restoring-default') {
        Stop-Refused 'A restore was interrupted. Run restore-default-profile.cmd first.'
    } elseif ($state -ne 'stashing-default') {
        Stop-Refused ('Unknown swap state "' + $state + '" in ' + $script:StatePath + '. Nothing was changed.')
    }

    Assert-CredentialShapes
    Assert-NoBlockers

    if ($resumeOnly) {
        Say 'Finishing an interrupted resume of the saved tutorial profile.'
        Invoke-Phase 'tutorial' 'live' $true
        Invoke-Phase 'tutorial' 'live' $false
        Say 'Done. The tutorial profile is active.'
        return
    }

    Say 'Swap to the tutorial profile. Your default profile is set aside by renaming; nothing is deleted.'
    Say 'Will set aside:'
    Show-Moves 'live' 'default'

    $installRoot = Read-InstallRoot (Get-BootstrapFile 'live')
    if ($null -eq $installRoot) {
        $installRoot = Read-InstallRoot (Get-BootstrapFile 'default')
    }
    if ($null -ne $installRoot -and -not (Test-Inside $installRoot ($script:LocalData + '\vjumpkung-comfyui'))) {
        Say ('Warning: Your real install root ' + $installRoot + ' is not hidden. Do not choose or open it during the tutorial.')
    }
    if ([Environment]::GetEnvironmentVariable('VJUMPKUNG_COMFYUI_HOME')) {
        Say 'Warning: VJUMPKUNG_COMFYUI_HOME is set; debug builds ignore this swap.'
    }
    Say 'Note: the folder picker''s recent-folder history is not swapped.'

    $mode = 'new'
    if ($tutorialPresent.Count -gt 0) {
        Say 'A saved tutorial profile exists:'
        foreach ($path in $tutorialPresent) {
            Say ('  ' + $path)
        }
        $answer = Read-Answer '[R]esume saved tutorial / [F]resh start (permanently deletes it) / anything else cancels: '
        if (Test-Answer $answer @('r')) {
            $mode = 'resume'
        } elseif (Test-Answer $answer @('f')) {
            foreach ($item in $script:Items) {
                $path = Get-ItemPath $item 'tutorial'
                if ($item.Kind -eq 'dir' -and [IO.Directory]::Exists($path) -and (Test-ReparsePoint $path)) {
                    Stop-Refused ('Refusing a fresh start because ' + $path + ' is a link (junction or symlink). Nothing was changed.')
                }
            }
            $confirm = Read-Answer 'Type DELETE to permanently delete the saved tutorial profile: '
            if ($null -eq $confirm -or $confirm -cne 'DELETE') {
                Stop-Cancelled
            }
            $mode = 'fresh'
        } else {
            Stop-Cancelled
        }
    } else {
        $answer = Read-Answer 'Swap now? [y/N]: '
        if (-not (Test-Answer $answer @('y', 'yes'))) {
            Stop-Cancelled
        }
    }

    Invoke-Phase 'live' 'default' $true

    $discards = New-Object System.Collections.ArrayList
    if ($mode -eq 'fresh') {
        $savedRoot = Read-InstallRoot (Get-BootstrapFile 'tutorial')
        if ($null -ne $savedRoot) {
            Say ('Install root of the saved tutorial profile, left in place: ' + $savedRoot)
        }
        $stamp = Get-Date -Format 'yyyyMMddHHmmss'
        foreach ($item in $script:Items) {
            $path = Get-ItemPath $item 'tutorial'
            if ($item.Kind -eq 'dir') {
                if ([IO.Directory]::Exists($path) -or [IO.File]::Exists($path)) {
                    $discard = $item.Live + '.profile-discard-' + $stamp
                    [void](Move-Directory $path $discard $false)
                    [void]$discards.Add($discard)
                }
            } elseif ([VjProfileSwap.Credentials]::Probe($path) -eq 1) {
                Say ('Deleting saved tutorial token ' + $path)
                [VjProfileSwap.Credentials]::Delete($path)
            }
        }
    }

    Set-SwapState 'stashing-default'
    Invoke-Phase 'live' 'default' $false
    Set-SwapState 'tutorial'
    if ($mode -eq 'resume') {
        Invoke-Phase 'tutorial' 'live' $false
    }

    Say ''
    if ($mode -eq 'resume') {
        Say 'Done. The saved tutorial profile is active.'
    } else {
        Say 'Done. The tutorial profile is active; the app will open at first-run setup.'
    }
    Say 'Next: launch the app. When you finish, close the app and run scripts\restore-default-profile.cmd.'

    foreach ($discard in $discards) {
        Remove-DiscardDirectory $discard
    }
}

function Invoke-ToDefault {
    $state = Get-SwapState
    $tutorialPresent = @(Get-PresentStash 'tutorial')
    $defaultPresent = @(Get-PresentStash 'default')
    if ($null -eq $state) {
        if ($defaultPresent.Count -gt 0) {
            Say 'Found a saved default profile but no swap state:'
            foreach ($path in $defaultPresent) {
                Say ('  ' + $path)
            }
            Stop-Refused 'This is inconsistent, so nothing was changed. Inspect those items, then rename them back or remove them by hand.'
        }
        Say 'Default profile is already active.'
        if ($tutorialPresent.Count -gt 0) {
            Say 'A saved tutorial profile exists (*.profile-tutorial); run swap-to-tutorial-profile.cmd to resume it.'
        }
        return
    }
    if ($state -ne 'tutorial' -and $state -ne 'stashing-tutorial' -and $state -ne 'stashing-default' -and $state -ne 'restoring-default') {
        Stop-Refused ('Unknown swap state "' + $state + '" in ' + $script:StatePath + '. Nothing was changed.')
    }
    $saveTutorial = ($state -eq 'tutorial' -or $state -eq 'stashing-tutorial')

    Assert-CredentialShapes
    Assert-NoBlockers

    Say 'Restore the default profile.'
    if ($saveTutorial) {
        Say 'Will set aside the tutorial profile as *.profile-tutorial:'
        Show-Moves 'live' 'tutorial'
        Invoke-Phase 'live' 'tutorial' $true
    }
    Say 'Will restore your default profile:'
    Show-Moves 'default' 'live'
    if (-not $saveTutorial) {
        Invoke-Phase 'default' 'live' $true
    }
    $answer = Read-Answer 'Restore now? [y/N]: '
    if (-not (Test-Answer $answer @('y', 'yes'))) {
        Stop-Cancelled
    }

    if ($saveTutorial) {
        Set-SwapState 'stashing-tutorial'
        Invoke-Phase 'live' 'tutorial' $false
    }
    Set-SwapState 'restoring-default'
    Invoke-Phase 'default' 'live' $false
    Remove-SwapState

    Say ''
    Say 'Done. The default profile is active.'
    if (@(Get-PresentStash 'tutorial').Count -gt 0) {
        Say 'Your tutorial profile is saved as *.profile-tutorial; the next swap offers Resume or Fresh.'
    }
}

function Invoke-Main {
    Add-Type -TypeDefinition $helperSource -Language CSharp
    if ($To -eq 'tutorial') {
        Invoke-ToTutorial
    } else {
        Invoke-ToDefault
    }
}

$exitCode = 0
try {
    Invoke-Main
} catch [System.OperationCanceledException] {
    Say $_.Exception.Message
    $exitCode = 1
} catch [System.InvalidOperationException] {
    Say $_.Exception.Message
    $exitCode = 1
} catch {
    Say ('Unexpected error (' + $_.Exception.GetType().Name + '): ' + $_.Exception.Message)
    Say 'Nothing is lost. Run this script again, or run restore-default-profile.cmd to go back.'
    $exitCode = 1
}
Invoke-Pause
exit $exitCode
