# Installs wolf for the current user on Windows, without administrator rights. In PowerShell:
#
#   irm https://github.com/realfamousbae/wolfpack/releases/latest/download/install.ps1 | iex
#
# It downloads the archive of the release, checks it against SHA256SUMS.txt of the same release,
# puts wolf.exe into %LOCALAPPDATA%\Programs\wolfpack\bin (the directory of the setup program),
# adds that directory to the user's PATH and runs `wolf setup`, which installs Node.js,
# TypeScript, Go and Rust into %USERPROFILE%\.wolfpack.
#
# The script reads these environment variables (set them before `irm ... | iex`):
#
#   WOLFPACK_VERSION         install this release instead of the latest one, for example 0.1.0
#   WOLFPACK_INSTALL_DIR     install wolf.exe into this directory
#   WOLFPACK_NO_MODIFY_PATH  do not change the user's PATH
#   WOLFPACK_NO_SETUP        do not run `wolf setup`
#   WOLFPACK_DOWNLOAD_URL    take the files from here (a mirror, or a local directory) instead of
#                            GitHub
#
# It works in Windows PowerShell 5.1 and in PowerShell 7. Everything is inside one function, so a
# download that breaks off halfway runs nothing, and an error never closes the user's window.

function Install-Wolfpack {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue' # the progress bar makes downloads much slower in 5.1

    $releases = 'https://github.com/realfamousbae/wolfpack/releases'
    $asset = 'wolfpack-windows-x64.zip'

    function Say([string]$Message) {
        Write-Host "wolfpack: $Message"
    }

    if ($PSVersionTable.PSVersion.Major -lt 5) {
        throw 'wolfpack: this installer needs PowerShell 5.1 or newer'
    }
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
        throw 'wolfpack: this installer is for Windows; on Linux and macOS run: curl -fsSL https://github.com/realfamousbae/wolfpack/releases/latest/download/install.sh | sh'
    }
    if (-not [Environment]::Is64BitOperatingSystem) {
        throw 'wolfpack: there is no build of wolf for 32-bit Windows'
    }
    $arch = $env:PROCESSOR_ARCHITEW6432
    if (-not $arch) { $arch = $env:PROCESSOR_ARCHITECTURE }
    if ($arch -eq 'ARM64') {
        Say 'this is Windows on ARM; the x64 build of wolf runs under emulation'
    }

    # PowerShell 5.1 may default to TLS 1.0, which GitHub refuses.
    try {
        [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    } catch {
        # An old .NET without TLS 1.2: the download below reports the problem.
    }
    # A corporate proxy that needs the Windows sign-in.
    try {
        [Net.WebRequest]::DefaultWebProxy.Credentials = [Net.CredentialCache]::DefaultNetworkCredentials
    } catch {
    }

    $version = "$env:WOLFPACK_VERSION".Trim().TrimStart('v')
    if ($env:WOLFPACK_DOWNLOAD_URL) {
        $base = $env:WOLFPACK_DOWNLOAD_URL.TrimEnd('/', '\')
    } elseif ($version) {
        $base = "$releases/download/v$version"
    } else {
        $base = "$releases/latest/download"
    }
    if ($env:WOLFPACK_INSTALL_DIR) {
        $dir = $env:WOLFPACK_INSTALL_DIR
    } else {
        $dir = Join-Path $env:LOCALAPPDATA 'Programs\wolfpack\bin'
    }

    function Get-File([string]$From, [string]$To) {
        if (Test-Path -LiteralPath $From) {
            Copy-Item -LiteralPath $From -Destination $To
            return
        }
        for ($try = 1; $try -le 3; $try++) {
            try {
                Invoke-WebRequest -Uri $From -OutFile $To -UseBasicParsing
                return
            } catch {
                if ($try -eq 3) {
                    throw "wolfpack: cannot download $From ($($_.Exception.Message)); check the network connection, a proxy, or the version number"
                }
                Start-Sleep -Seconds (2 * $try)
            }
        }
    }

    $work = Join-Path ([IO.Path]::GetTempPath()) ("wolfpack-" + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $work | Out-Null
    try {
        Say "downloading $base/$asset"
        $zip = Join-Path $work $asset
        Get-File "$base/$asset" $zip
        $sums = Join-Path $work 'SHA256SUMS.txt'
        Get-File "$base/SHA256SUMS.txt" $sums

        $want = $null
        foreach ($line in Get-Content -LiteralPath $sums) {
            $parts = $line -split '\s+', 2
            if ($parts.Count -eq 2 -and $parts[1].TrimStart('*') -eq $asset) { $want = $parts[0] }
        }
        if (-not $want) { throw "wolfpack: SHA256SUMS.txt of the release does not list $asset" }
        $got = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash
        if ($got -ne $want) {
            throw "wolfpack: the download of $asset is damaged (SHA-256 $got, expected $want); try again"
        }
        Say 'checked the SHA-256 sum'

        $unpacked = Join-Path $work 'unpacked'
        Expand-Archive -LiteralPath $zip -DestinationPath $unpacked
        $exe = Join-Path $unpacked 'wolf.exe'
        if (-not (Test-Path -LiteralPath $exe)) { throw "wolfpack: $asset does not contain wolf.exe" }

        New-Item -ItemType Directory -Force -Path $dir | Out-Null
        $target = Join-Path $dir 'wolf.exe'
        # A running wolf.exe cannot be overwritten, but it can be renamed out of the way.
        if (Test-Path -LiteralPath $target) {
            $old = "$target.old"
            Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue
            try {
                Move-Item -LiteralPath $target -Destination $old -Force
            } catch {
                throw "wolfpack: cannot replace $target; close the programs that use wolf and try again"
            }
        }
        Copy-Item -LiteralPath $exe -Destination $target
        Remove-Item -LiteralPath "$target.old" -Force -ErrorAction SilentlyContinue
        try {
            $installed = & $target --version
        } catch {
            $installed = $null
        }
        if (-not $installed -or $LASTEXITCODE -ne 0) {
            throw 'wolfpack: the installed wolf.exe does not run; an antivirus program may have blocked it'
        }
        Say "installed $installed into $dir"
    } finally {
        Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
    }

    if (-not $env:WOLFPACK_NO_MODIFY_PATH) {
        # The registry value, not `setx`, which cuts PATH at 1024 characters. The raw value keeps
        # entries like %USERPROFILE%\bin unexpanded.
        $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
        try {
            $path = $key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
            $entries = @($path -split ';' | Where-Object { $_ -ne '' })
            if ($entries -notcontains $dir) {
                $kind = [Microsoft.Win32.RegistryValueKind]::ExpandString
                $key.SetValue('Path', (($entries + $dir) -join ';'), $kind)
                # Tells running programs (Explorer, new terminals) that PATH changed.
                [Environment]::SetEnvironmentVariable('WOLFPACK_PATH_CHANGED', '1', 'User')
                [Environment]::SetEnvironmentVariable('WOLFPACK_PATH_CHANGED', $null, 'User')
                Say "added $dir to the user's PATH"
            }
        } finally {
            $key.Close()
        }
        if (($env:Path -split ';') -notcontains $dir) {
            $env:Path = "$dir;$env:Path"
        }
    }

    if (-not $env:WOLFPACK_NO_SETUP) {
        Say 'installing the tools of the targets with `wolf setup` (about 1 GB, into %USERPROFILE%\.wolfpack)'
        & (Join-Path $dir 'wolf.exe') setup
        if ($LASTEXITCODE -ne 0) {
            Say 'some tools could not be installed; run `wolf setup` again later'
        }
    }

    Write-Host ''
    Say 'done. Open a new terminal, then try:'
    Write-Host '    wolf new hello; cd hello; wolf run'
}

Install-Wolfpack
