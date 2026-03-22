import { useEffect, memo } from 'react'

interface HelpModalProps {
  onClose: () => void
}

export const HelpModal = memo(function HelpModal({ onClose }: HelpModalProps) {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [onClose])

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70" onClick={onClose}>
      <div
        className="bg-bg border border-border rounded-lg max-w-2xl w-full mx-3 sm:mx-4 max-h-[85vh] overflow-y-auto p-4 sm:p-6"
        onClick={e => e.stopPropagation()}
      >
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-accent font-bold text-lg">Voice Commands</h2>
          <button onClick={onClose} className="text-text-dim hover:text-text text-lg">&times;</button>
        </div>

        <Section title="Session Management">
          <Row cmd="new session" desc="Start a fresh conversation session" />
          <Row cmd="list sessions" desc="List all sessions by number" />
          <Row cmd='switch to <name>' desc="Switch to a session by name" />
          <Row cmd="<codename>" desc="Say a session codename directly to switch" />
          <Row cmd='rename session <name>' desc="Rename the current session" />
          <Aliases>new conversation, reset session, clear context, start over, fresh start, show sessions, go to session, name session</Aliases>
        </Section>

        <Section title="Voice Control">
          <Row cmd="voice on" desc="Enable spoken responses (short, 1-2 sentences)" />
          <Row cmd="voice off" desc="Disable spoken responses (full text replies)" />
          <Aliases>speech on, speech off</Aliases>
        </Section>

        <Section title="Recording">
          <Row cmd="Hold pedal" desc="Start recording your voice" />
          <Row cmd="Release pedal" desc="Stop recording and send to AI" />
          <Row cmd="ignore this" desc="Cancel the current recording" />
          <Aliases>never mind, forget it, scratch that</Aliases>
        </Section>

        <Section title="Other">
          <Row cmd="help" desc="Show this help page" />
          <Row cmd="yes / no" desc="Confirm or cancel a pending action" />
          <Aliases>yeah, yep, confirm, do it, nope, cancel, never mind</Aliases>
        </Section>
      </div>
    </div>
  )
})

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="mb-4">
      <h3 className="text-text font-bold text-sm mb-2 border-b border-border pb-1">{title}</h3>
      <table className="w-full text-sm">
        <tbody>{children}</tbody>
      </table>
    </div>
  )
}

function Row({ cmd, desc }: { cmd: string; desc: string }) {
  return (
    <tr>
      <td className="text-accent pr-4 py-0.5 whitespace-nowrap">{cmd}</td>
      <td className="text-text-dim py-0.5">{desc}</td>
    </tr>
  )
}

function Aliases({ children }: { children: string }) {
  return (
    <tr>
      <td colSpan={2} className="text-text-dim text-xs pt-1 pb-2 opacity-60">
        Aliases: {children}
      </td>
    </tr>
  )
}
