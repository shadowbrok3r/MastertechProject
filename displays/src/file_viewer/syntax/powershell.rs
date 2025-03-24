/// file_viewer/syntax/powershell.rs
use super::Syntax;
use std::collections::BTreeSet;

impl Syntax {
    pub fn powershell() -> Self {
        Syntax {
            language: "PowerShell",
            case_sensitive: false, // PowerShell is case-insensitive
            comment: "#",
            comment_multiline: ["<#", "#>"],
            hyperlinks: BTreeSet::from(["http", "https"]),
            keywords: BTreeSet::from([
                // Control Flow
                "if", "else", "elseif", "switch", "for", "foreach", "while", "do", "until",
                "break", "continue", "return",
                // Functions
                "function", "param", "begin", "process", "end",
                // Error Handling
                "try", "catch", "finally", "throw",
                // Script Keywords
                "workflow", "class", "enum", "exit", "trap", "using",
                // Variables
                "$true", "$false", "$null", "$_",
            ]),
            types: BTreeSet::from([
                // Data Types
                "[int]", "[string]", "[bool]", "[array]", "[hashtable]", "[datetime]",
                "[xml]", "[psobject]", "[scriptblock]",
                // Environment Variables
                "$env:", "$home", "$pid", "$pwd", "$error", "$profile", "$psversiontable",
            ]),
            special: BTreeSet::from([
                // Operators (as in the PowerShell script's TokenFlags check)
                "eq", "ne", "gt", "ge", "lt", "le", "like", "notlike", "match", "notmatch",
                "contains", "notcontains", "replace", "split", "join", "and", "or", "not",
                "band", "bor", "bxor", "shl", "shr",
                // Cmdlets (expanded list based on common usage)
                "Get-Command", "Get-Help", "Get-Process", "Get-Service", "Set-Variable",
                "Get-Variable", "Remove-Variable", "Write-Host", "Write-Output", "Write-Error",
                "Read-Host", "Start-Process", "Stop-Process", "Test-Path", "New-Object",
                "New-Item", "Remove-Item", "Copy-Item", "Move-Item", "Rename-Item",
                "Get-Content", "Set-Content", "Add-Content", "Clear-Content", "Get-ChildItem",
                "Get-Item", "Set-Item", "Get-ItemProperty", "Set-ItemProperty",
                // Aliases
                "ls", "dir", "cp", "mv", "rm", "cat", "more", "gci", "gc", "echo", "write", "sort",
            ]),
        }
    }
}