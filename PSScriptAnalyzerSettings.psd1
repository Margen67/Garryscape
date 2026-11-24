# SPDX-License-Identifier: AGPL-3.0-or-later
@{
    ExcludeRules = @(
        # The supplied compatibility targets predate the PowerShell 7.4 minimum.
        'PSUseCompatibleCmdlets'
        'PSUseCompatibleCommands'
        'PSUseCompatibleSyntax'
        'PSUseCompatibleTypes'
        # The launcher uses filesystem and path methods that require FullLanguage.
        'PSUseConstrainedLanguageMode'
        # EditorConfig specifies UTF-8 without a BOM.
        'PSUseBOMForUnicodeEncodedFile'
        # Prefer the ! negation operator.
        'PSAvoidExclaimOperator'
    )
    Rules        = @{
        PSAlignAssignmentStatement                = @{ Enable = $true }
        PSAvoidLongLines                          = @{ Enable = $true }
        PSAvoidSemicolonsAsLineTerminators        = @{ Enable = $true }
        PSAvoidUsingDoubleQuotesForConstantString = @{ Enable = $true }
        PSPlaceCloseBrace                         = @{ Enable = $true }
        PSPlaceOpenBrace                          = @{ Enable = $true }
        PSUseConsistentIndentation                = @{ Enable = $true }
        PSUseConsistentParameterSetName           = @{ Enable = $true }
        PSUseConsistentParametersKind             = @{ Enable = $true }
        PSUseConsistentWhitespace                 = @{
            Enable                                  = $true
            # Alignment intentionally adds spaces before hashtable assignment operators.
            IgnoreAssignmentOperatorInsideHashTable = $true
        }
        PSUseCorrectCasing                        = @{ Enable = $true }
        PSUseSingleValueFromPipelineParameter     = @{ Enable = $true }
    }
}
