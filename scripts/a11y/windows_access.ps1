param([int]$AppProcessId, [string]$Operation, [string]$Name = '', [string]$Value = '')
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$condition = [System.Windows.Automation.PropertyCondition]::new(
    [System.Windows.Automation.AutomationElement]::ProcessIdProperty, $AppProcessId)
$root = [System.Windows.Automation.AutomationElement]::RootElement.FindFirst(
    [System.Windows.Automation.TreeScope]::Children, $condition)
if ($null -eq $root) { '[]'; exit 0 }
$nodes = $root.FindAll([System.Windows.Automation.TreeScope]::Subtree,
    [System.Windows.Automation.Condition]::TrueCondition)
if ($Operation -eq 'tree') {
    $result = @($nodes | ForEach-Object {
        $range = $null
        $text = $null
        $nodeValue = $null
        if ($_.TryGetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern, [ref]$range)) {
            $nodeValue = $range.Current.Value
        } elseif ($_.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$text)) {
            $nodeValue = $text.Current.Value
        }
        $role = $_.Current.ControlType.ProgrammaticName
        if ($role -eq 'ControlType.ProgressBar') { $role = 'progress' }
        if ($role -eq 'ControlType.Slider') { $role = 'slider' }
        @{ name = $_.Current.Name; role = $role; value = $nodeValue; enabled = $_.Current.IsEnabled }
    })
    ConvertTo-Json -InputObject $result -Depth 5 -Compress
    exit 0
}
$target = @($nodes | Where-Object { $_.Current.Name -eq $Name })
if ($target.Count -ne 1) { throw "Expected one native node named $Name, found $($target.Count)" }
$pattern = $null
$success = $false
if ($Operation -eq 'activate') {
    if ($target[0].TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
        try { $pattern.Invoke(); $success = $true }
        catch [System.Windows.Automation.ElementNotEnabledException] { $success = $false }
    }
} elseif ($Operation -eq 'value') {
    if ($target[0].TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
        $pattern.SetValue($Value); $success = $true
    } elseif ($target[0].TryGetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern, [ref]$pattern)) {
        $pattern.SetValue([double]$Value); $success = $true
    }
} else { throw "Unknown operation $Operation" }
ConvertTo-Json -InputObject $success -Compress
