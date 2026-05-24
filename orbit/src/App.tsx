import { useEffect } from 'react'
import { usePolling } from './api/hooks'
import { useUi } from './store/ui'
import { Background } from './components/Background'
import { Header } from './components/Header'
import { Footer } from './components/Footer'
import { CredentialStatusRow } from './components/CredentialStatusRow'
import { CapacityRow } from './components/CapacityRow'
import { VesselGrid } from './components/VesselGrid'
import { DeployDialog } from './components/DeployDialog'
import { DestroyDialog } from './components/DestroyDialog'
import { CredentialWizard } from './components/CredentialWizard'
import { LogsDrawer } from './components/LogsDrawer'
import { ClaudeMdEditor } from './components/ClaudeMdEditor'
import { Toaster } from './components/Toaster'

export default function App() {
  usePolling()

  // Auto-open the wizard on first run if NO credentials are staged at all.
  const initialLoad = useUi((s) => s.initialLoad)
  const credentials = useUi((s) => s.credentials)
  const githubAccounts = useUi((s) => s.githubAccounts)
  const wizardOpen = useUi((s) => s.wizardOpen)
  const openWizard = useUi((s) => s.openWizard)

  useEffect(() => {
    if (initialLoad) return
    if (wizardOpen) return
    const allKnown = credentials !== null && githubAccounts !== null
    if (!allKnown) return
    const claudeMissing = !credentials.present
    const githubMissing = githubAccounts.every((a) => !a.present)
    if (claudeMissing && githubMissing) openWizard(true)
    // run once after initial load resolves
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [initialLoad])

  return (
    <>
      <Background />
      <div className="relative z-10 flex min-h-screen flex-col">
        <Header />
        <main className="flex-1 px-6 lg:px-10 max-w-[1600px] mx-auto w-full">
          <CredentialStatusRow />
          <CapacityRow />
          <VesselGrid />
        </main>
        <Footer />
      </div>
      <DeployDialog />
      <DestroyDialog />
      <CredentialWizard />
      <LogsDrawer />
      <ClaudeMdEditor />
      <Toaster />
    </>
  )
}
