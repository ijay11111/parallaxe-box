# Static script. Runtime inputs arrive as JSON on stdin, never as PowerShell source.
$ErrorActionPreference = 'Stop'
$j = [Console]::In.ReadToEnd() | ConvertFrom-Json
$label = 'parallaxe-block-' + $j.tag
$name = $label + '-' + $j.run
$rules = @(Get-NetFirewallRule -PolicyStore PersistentStore -All | Where-Object { $_.Name -eq $name -or $_.DisplayName -eq $label })
if ($j.op -eq 'create') {
    if ($rules.Count) { throw 'rule already exists' }
    New-NetFirewallRule -PolicyStore PersistentStore -Name $name -DisplayName $label -Description $j.marker -Direction Outbound -Action Block -Enabled True -Profile Any -RemoteAddress $j.ips | Out-Null
    return
}
if (!$rules.Count) { 'ABSENT'; return }
if ($rules.Count -ne 1) { 'FOREIGN'; return }
$r = $rules[0]
if ($r.Name -ne $name -or $r.Description -cne $j.marker -or $r.Direction -ne 'Outbound' -or $r.Action -ne 'Block' -or $r.Enabled -ne 'True' -or $r.Profile -ne 'Any') { 'FOREIGN'; return }
$addresses = ($r | Get-NetFirewallAddressFilter).RemoteAddress
$actual = @()
foreach ($address in $addresses) {
    $ip = $null
    $parts = $address -split '/'
    if ($parts.Count -gt 2 -or ![IPAddress]::TryParse($parts[0], [ref]$ip)) { 'FOREIGN'; return }
    if ($parts.Count -eq 2 -and $parts[1] -ne [string]($ip.GetAddressBytes().Length * 8)) { 'FOREIGN'; return }
    $actual += $ip.ToString()
}
$actual = @($actual | Sort-Object -Unique)
$expected = @($j.ips | ForEach-Object { ([IPAddress]::Parse($_)).ToString() } | Sort-Object -Unique)
if (!$actual.Count -or (Compare-Object $actual $expected)) { 'FOREIGN'; return }
if ($j.op -eq 'remove') { $r | Remove-NetFirewallRule -Confirm:$false }
'OWNED'
