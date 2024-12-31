use super::Syntax;
use std::collections::BTreeSet;

impl Syntax {
    pub fn powershell() -> Self {
        Syntax {
            language: "PowerShell",
            case_sensitive: true,
            comment: "#",
            hyperlinks: BTreeSet::from(["http", "https"]),
            keywords: BTreeSet::from([
                // Control Flow
                "if", "else", "elseif", "switch", "for", "foreach", "while", "do", "until", "break", "continue", "return",
                // Functions
                "function", "param", "begin", "process", "end",
                // Operators
                "eq", "ne", "gt", "ge", "lt", "le", "like", "notlike", "match", "notmatch", "contains", "notcontains",
                "replace", "split", "join", "and", "or", "not", "band", "bor", "bxor", "shl", "shr",
                // Error Handling
                "try", "catch", "finally", "throw",
                // Data Types
                "int", "string", "bool", "array", "hashtable", "datetime", "xml", "psobject", "scriptblock",
                // Script Keywords
                "workflow", "class", "enum", "exit", "trap", "using",
                // Variables
                "$true", "$false", "$null", "$_"
            ]),
            comment_multiline: ["<#", "#>"],
            types: BTreeSet::from([
                // Environment Variables
                "$env:", "$home", "$pid", "$pwd", "$error", "$profile", "$psversiontable",
                // PowerShell Types
                "[int]", "[string]", "[bool]", "[array]", "[hashtable]", "[datetime]", "[xml]", "[psobject]", "[scriptblock]",
            ]),
            special: BTreeSet::from([
                // Cmdlets
                "Get-Command", "Get-Help", "Get-Process", "Get-Service", "Set-Variable", "Get-Variable", "Remove-Variable",
                "Write-Host", "Write-Output", "Write-Error", "Read-Host", "Start-Process", "Stop-Process", "Test-Path",
                "New-Object", "New-Item", "Remove-Item", "Copy-Item", "Move-Item", "Rename-Item", "Get-Content",
                "Set-Content", "Add-Content", "Clear-Content", "Get-ChildItem", "Get-Item", "Set-Item", "Get-ItemProperty",
                "Set-ItemProperty", "Get-Acl", "Set-Acl", "Invoke-Command", "Invoke-Expression", "Import-Module",
                "Export-ModuleMember", "Connect-Session", "Disconnect-Session", "Move-Duplicates", "Join-Path", "Out-Null", 
                "Path", "Recurse", "Length", "Hash",
                // Aliases
                "ls", "dir", "cp", "mv", "rm", "cat", "more", "gci", "gc", "echo", "write", "sort", "cd", "pwd",
            ]),
        }
    }
}