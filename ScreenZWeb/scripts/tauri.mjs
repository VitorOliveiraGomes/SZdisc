import { execFileSync, spawn } from 'node:child_process'
import { existsSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const cli = fileURLToPath(new URL('../node_modules/@tauri-apps/cli/tauri.js', import.meta.url))
const root = fileURLToPath(new URL('../../', import.meta.url))
const env = { ...process.env }
if (process.platform === 'win32' && !env.RUSTUP_TOOLCHAIN) {
  env.RUSTUP_TOOLCHAIN = 'stable-msvc'
}
const portableSetup = path.join(root, 'target', 'toolchain', 'msvc', 'setup_x64.bat')
if (process.platform === 'win32' && existsSync(portableSetup)) {
  const environment = execFileSync(
    'cmd.exe',
    ['/d', '/s', '/c', `call "${portableSetup}" >nul && set`],
    {
      env,
      encoding: 'utf8',
      windowsHide: true,
      windowsVerbatimArguments: true,
    },
  )
  for (const line of environment.split(/\r?\n/)) {
    const separator = line.indexOf('=')
    if (separator > 0) env[line.slice(0, separator)] = line.slice(separator + 1)
  }
}

const child = spawn(process.execPath, [cli, ...process.argv.slice(2)], {
  cwd: root,
  env,
  stdio: 'inherit',
})
child.on('error', (error) => {
  console.error(error.message)
  process.exitCode = 1
})
child.on('exit', (code) => {
  process.exitCode = code ?? 1
})
