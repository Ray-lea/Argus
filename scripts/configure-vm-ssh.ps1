param(
    [Parameter(Mandatory=$true)][string]$PublicKey,
    [Parameter(Mandatory=$true)][string]$ClientAddress,
    [string]$Account = 'ArgusLab'
)
$ErrorActionPreference = 'Stop'
$manufacturer = (Get-ItemProperty 'HKLM:\HARDWARE\DESCRIPTION\System\BIOS').SystemManufacturer
if ($manufacturer -notlike '*QEMU*') { throw 'This setup is restricted to the dedicated QEMU VM.' }
if ($PublicKey -notmatch '^ssh-ed25519 [A-Za-z0-9+/=]+(?: [a-zA-Z0-9_.-]+)?$') { throw 'Expected one Ed25519 public key.' }
if ($ClientAddress -notmatch '^192\.168\.\d{1,3}\.\d{1,3}$') { throw 'Expected the test LAN client address.' }
if ($Account -notmatch '^[a-zA-Z0-9_-]+$') { throw 'Invalid test account name.' }
if ((Get-WindowsCapability -Online -Name 'OpenSSH.Server~~~~0.0.1.0').State -ne 'Installed') { throw 'Install the Windows OpenSSH Server capability first.' }
$sshFolder = Join-Path $env:ProgramData 'ssh'
New-Item -ItemType Directory -Path $sshFolder -Force | Out-Null
$authorized = Join-Path $sshFolder 'administrators_authorized_keys'
if (Test-Path -LiteralPath $authorized) { Copy-Item -LiteralPath $authorized -Destination ($authorized + '.before-argus-' + (Get-Date -Format 'yyyyMMddHHmmss')) }
Set-Content -LiteralPath $authorized -Value $PublicKey -Encoding Ascii
& icacls.exe $authorized /inheritance:r /grant '*S-1-5-18:(F)' '*S-1-5-32-544:(F)' | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Failed to restrict the authorized key ACL.' }
$config = Join-Path $sshFolder 'sshd_config'
if (Test-Path -LiteralPath $config) { Copy-Item -LiteralPath $config -Destination ($config + '.before-argus-' + (Get-Date -Format 'yyyyMMddHHmmss')) }
$lines = @(
    '# Argus disposable VM access',
    'Port 22', 'AddressFamily inet', 'PubkeyAuthentication yes', 'PasswordAuthentication no',
    ('AllowUsers ' + $Account.ToLowerInvariant()),
    'AllowTcpForwarding local', 'GatewayPorts no', 'PermitOpen 127.0.0.1:18762',
    'Subsystem sftp sftp-server.exe', 'Match Group administrators',
    '    AuthorizedKeysFile __PROGRAMDATA__/ssh/administrators_authorized_keys'
)
Set-Content -LiteralPath $config -Value $lines -Encoding Ascii
& "$env:WINDIR\System32\OpenSSH\ssh-keygen.exe" -A
& "$env:WINDIR\System32\OpenSSH\sshd.exe" -t
if ($LASTEXITCODE -ne 0) { throw 'The SSH server configuration is invalid.' }
$rule = Get-NetFirewallRule -Name 'OpenSSH-Server-In-TCP' -ErrorAction SilentlyContinue
if ($rule) { $rule | Set-NetFirewallRule -Enabled True -Profile Any -RemoteAddress $ClientAddress }
else { New-NetFirewallRule -Name 'OpenSSH-Server-In-TCP' -DisplayName 'OpenSSH Server' -Profile Any -Direction Inbound -Protocol TCP -LocalPort 22 -Action Allow -RemoteAddress $ClientAddress | Out-Null }
New-ItemProperty -Path 'HKLM:\SOFTWARE\OpenSSH' -Name DefaultShell -Value "$env:WINDIR\System32\WindowsPowerShell\v1.0\powershell.exe" -PropertyType String -Force | Out-Null
Set-Service -Name sshd -StartupType Automatic
Restart-Service -Name sshd
Get-Service -Name sshd | Select-Object Name, Status, StartType
& "$env:WINDIR\System32\OpenSSH\ssh-keygen.exe" -lf (Join-Path $sshFolder 'ssh_host_ed25519_key.pub')
