const fs = require('fs');
const path = require('path');

const SUPPORTED_TARGET_PLATFORMS = new Set(['win32', 'linux']);
const TARGET_PLATFORM_ALIASES = Object.freeze({
    win: 'win32',
    windows: 'win32',
    win32: 'win32',
    linux: 'linux'
});

function resolveTargetPlatform(targetPlatform = process.env.REFEREE_TARGET_PLATFORM || process.platform) {
    const normalized = TARGET_PLATFORM_ALIASES[String(targetPlatform || '').toLowerCase()] ||
        String(targetPlatform || '').toLowerCase();

    if (!SUPPORTED_TARGET_PLATFORMS.has(normalized)) {
        throw new Error(`Unsupported binary target platform: ${targetPlatform}`);
    }

    return normalized;
}

/**
 * Detects whether the current Linux system uses deb or rpm packages.
 * Returns "rpm" for Red Hat / Fedora / SUSE families, "deb" otherwise.
 */
function detectLinuxPackageType() {
    const rpmReleaseFiles = [
        '/etc/redhat-release',
        '/etc/fedora-release',
        '/etc/centos-release',
        '/etc/suse-release',
        '/etc/opensuse-release',
    ];
    for (const f of rpmReleaseFiles) {
        if (fs.existsSync(f)) return 'rpm';
    }

    try {
        const osRelease = fs.readFileSync('/etc/os-release', 'utf8');
        const rpmIds = ['rhel', 'fedora', 'centos', 'suse', 'opensuse'];
        for (const line of osRelease.split('\n')) {
            const [key, val] = line.split('=');
            if (key === 'ID' || key === 'ID_LIKE') {
                const v = (val || '').replace(/"/g, '').toLowerCase();
                if (rpmIds.some(id => v.includes(id))) return 'rpm';
            }
        }
    } catch (_) { /* not Linux or file missing */ }

    return 'deb';
}

function getPlatformBinaries(targetPlatform = resolveTargetPlatform()) {
    const resolvedTargetPlatform = resolveTargetPlatform(targetPlatform);
    const isWindows = resolvedTargetPlatform === 'win32';

    return {
        targetPlatform: resolvedTargetPlatform,
        isWindows,
        isLinux: resolvedTargetPlatform === 'linux',
        binaries: {
            nvencc: isWindows ? 'NVEncC64.exe' : 'NVEncC',
            vceenc: isWindows ? 'VCEEncC64.exe' : 'VCEEncC'
        }
    };
}

/**
 * Selects the appropriate Rigaya release asset for the current platform.
 * On Linux, chooses .deb or .rpm depending on the distro package manager.
 *
 * @param {Array} assets - Array of GitHub release asset objects (must have a `name` property).
 * @param {string} [targetPlatform] - Override platform detection.
 * @param {string} [packageType] - Override Linux package type ('deb' or 'rpm').
 */
function selectRigayaAsset(assets = [], targetPlatform = resolveTargetPlatform(), packageType) {
    const resolvedTargetPlatform = resolveTargetPlatform(targetPlatform);

    if (resolvedTargetPlatform === 'win32') {
        return assets.find(asset => /_x64\.7z$/i.test(asset.name || '')) || null;
    }

    const pkgType = packageType || detectLinuxPackageType();

    if (pkgType === 'rpm') {
        return assets.find(asset => /(?:amd64|x86_64)\.rpm$/i.test(asset.name || '')) || null;
    }

    return assets.find(asset => /(?:amd64|x86_64)\.deb$/i.test(asset.name || '')) || null;
}

function shouldInstallEncoderArtifact(fileName, targetPlatform = resolveTargetPlatform()) {
    const resolvedTargetPlatform = resolveTargetPlatform(targetPlatform);
    const extension = path.extname(fileName).toLowerCase();

    if (resolvedTargetPlatform === 'win32') {
        return ['.exe', '.dll', '.json'].includes(extension);
    }

    return fileName.includes('.so') || extension === '.json' || ['NVEncC', 'VCEEncC'].includes(fileName);
}

module.exports = {
    getPlatformBinaries,
    resolveTargetPlatform,
    detectLinuxPackageType,
    selectRigayaAsset,
    shouldInstallEncoderArtifact
};
