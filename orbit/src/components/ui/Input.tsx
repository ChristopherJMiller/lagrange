import { forwardRef, type InputHTMLAttributes, type TextareaHTMLAttributes } from 'react'
import { cn } from '../../lib/cn'

type InputProps = InputHTMLAttributes<HTMLInputElement> & {
  label?: string
  hint?: string
  error?: string
  mono?: boolean
}

export const Field = forwardRef<HTMLInputElement, InputProps>(function Field(
  { label, hint, error, mono = true, className, id, ...rest },
  ref,
) {
  const inputId = id || rest.name
  return (
    <label htmlFor={inputId} className="block">
      {label && (
        <div className="mb-1.5 flex items-baseline justify-between gap-2">
          <span className="text-[10px] uppercase tracking-widest text-dim">
            {label}
          </span>
          {error ? (
            <span className="text-[10px] uppercase tracking-wider text-red">{error}</span>
          ) : hint ? (
            <span className="text-[10px] uppercase tracking-wider text-dimmer">{hint}</span>
          ) : null}
        </div>
      )}
      <div
        className={cn(
          'group relative flex items-center border bg-bg/60',
          'transition-colors duration-150',
          error
            ? 'border-red/60 focus-within:border-red'
            : 'border-border focus-within:border-cyan',
        )}
      >
        <span className="px-2 text-cyan opacity-60 group-focus-within:opacity-100 select-none">
          ›
        </span>
        <input
          ref={ref}
          id={inputId}
          className={cn(
            'block w-full bg-transparent py-2 pr-3 text-sm text-text outline-none',
            'placeholder:text-dimmer',
            mono && 'font-mono',
            className,
          )}
          {...rest}
        />
      </div>
    </label>
  )
})

type TextareaProps = TextareaHTMLAttributes<HTMLTextAreaElement> & {
  label?: string
  hint?: string
  error?: string
}

export const Textarea = forwardRef<HTMLTextAreaElement, TextareaProps>(function Textarea(
  { label, hint, error, className, id, ...rest },
  ref,
) {
  const ta = id || rest.name
  return (
    <label htmlFor={ta} className="block">
      {label && (
        <div className="mb-1.5 flex items-baseline justify-between gap-2">
          <span className="text-[10px] uppercase tracking-widest text-dim">
            {label}
          </span>
          {error ? (
            <span className="text-[10px] uppercase tracking-wider text-red">{error}</span>
          ) : hint ? (
            <span className="text-[10px] uppercase tracking-wider text-dimmer">{hint}</span>
          ) : null}
        </div>
      )}
      <textarea
        ref={ref}
        id={ta}
        spellCheck={false}
        className={cn(
          'block w-full resize-y border bg-bg/60 px-3 py-2.5 font-mono text-xs leading-relaxed text-text',
          'placeholder:text-dimmer outline-none transition-colors duration-150',
          error
            ? 'border-red/60 focus:border-red'
            : 'border-border focus:border-cyan',
          className,
        )}
        {...rest}
      />
    </label>
  )
})
