param([Parameter(Mandatory, Position=0)][string]$Path)
$ErrorActionPreference = 'Stop'
if (!$env:AZURE_SIGNING_ENDPOINT -or !$env:AZURE_SIGNING_PUBLISHER) {
    throw 'Configure AZURE_SIGNING_ENDPOINT and AZURE_SIGNING_PUBLISHER on windows-signing.'
}
Import-Module ArtifactSigning -RequiredVersion 0.1.8
Invoke-ArtifactSigning -Endpoint $env:AZURE_SIGNING_ENDPOINT -CodeSigningAccountName rootatlas `
    -CertificateProfileName dial-desktop -Files (Resolve-Path -LiteralPath $Path).Path `
    -FileDigest SHA256 -TimestampRfc3161 'http://timestamp.acs.microsoft.com' -TimestampDigest SHA256
$sig = Get-AuthenticodeSignature -LiteralPath $Path
if ($sig.Status -ne 'Valid' -or !$sig.TimeStamperCertificate -or
    $sig.SignerCertificate.GetNameInfo('SimpleName', $false) -ne $env:AZURE_SIGNING_PUBLISHER) {
    throw "Expected a valid, timestamped release publisher signature on $Path."
}
