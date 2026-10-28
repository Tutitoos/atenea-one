// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useEffect, useId, useState } from 'react';
import { useForm } from '@tanstack/react-form';
import { Dialog as Primitive } from 'radix-ui';
import { motion, useReducedMotion } from 'motion/react';
import { useQueryClient, type UseQueryResult } from '@tanstack/react-query';
import { secondsSince } from './domain';
import { useClock } from './live';

type Variant = 'primary' | 'secondary' | 'danger' | 'link';
// Acepta atributos nativos; loading bloquea acciones repetidas sin perder el texto.
export function Button({ variant = 'secondary', loading = false, className = '', disabled, children, ...props }:
  React.ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; loading?: boolean }) {
  return <button type="button" {...props} className={`button ${variant} ${className}`}
    disabled={disabled || loading} aria-busy={loading || undefined}>
    {loading ? 'Enviando… ' : ''}{children}
  </button>;
}

// title es el h1 de la página; actions admite botones o enlaces.
export function PageHeader({ title, description, actions }: {
  title: string; description?: React.ReactNode; actions?: React.ReactNode;
}) {
  return <header className="page-heading"><div><h1>{title}</h1>
    {description && <p className="muted">{description}</p>}</div>
    {actions && <div className="actions">{actions}</div>}
  </header>;
}
export function Card({ title, children, className = '' }: {
  title?: string; children: React.ReactNode; className?: string;
}) {
  return <section className={`card ${className}`}>{title && <h2>{title}</h2>}{children}</section>;
}

type ControlProps = { id: string; 'aria-describedby'?: string; 'aria-invalid'?: true };
// children recibe los atributos que debe propagar al input/select/textarea.
export function Field({ label, help, error, children }: {
  label: string; help?: string; error?: string; children: (props: ControlProps) => React.ReactNode;
}) {
  const id = useId();
  const described = [help && `${id}-help`, error && `${id}-error`].filter(Boolean).join(' ');
  return <div className="field"><label htmlFor={id}>{label}</label>
    {children({ id, 'aria-describedby': described || undefined, 'aria-invalid': error ? true : undefined })}
    {help && <p id={`${id}-help`} className="muted field-help">{help}</p>}
    {error && <p id={`${id}-error`} className="field-error">{error}</p>}
  </div>;
}
export function StatusBadge({ tone = 'neutral', children }: {
  tone?: 'ok' | 'warn' | 'bad' | 'neutral'; children: React.ReactNode;
}) {
  return <span className={`badge badge-${tone}`}>{children}</span>;
}
export function EmptyState({ message, action }: { message: string; action: React.ReactNode }) {
  return <div className="empty"><p>{message}</p>{action}</div>;
}
// value solo se escribe en el portapapeles por una acción explícita; nunca se persiste.
export function CopyButton({ value, label = 'Copiar' }: { value: string; label?: string }) {
  const [message, setMessage] = useState('');
  return <span className="copy-control"><Button variant="link" aria-label={label} onClick={() => {
    void navigator.clipboard.writeText(value).then(() => setMessage('Copiado.'), () => setMessage('No se pudo copiar.'));
  }}><svg className="copy-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden="true">
    <rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V4H4v12h4"/>
  </svg></Button><span role="status">{message}</span></span>;
}
export function ShortId({ value }: { value: string }) {
  return <span className="short-id"><code title={value}>{value.slice(0, 8)}…</code>
    <CopyButton value={value} label={`Copiar UUID ${value}`}/></span>;
}

// Composición Radix: foco atrapado, Escape y retorno al disparador.
export function Dialog({ trigger, title, description, children, variant = 'secondary' }: {
  trigger: string; title: string; description: string; children: React.ReactNode; variant?: Variant;
}) {
  const reduce = useReducedMotion();
  return <Primitive.Root><Primitive.Trigger asChild><Button variant={variant}>{trigger}</Button></Primitive.Trigger>
    <Primitive.Portal><Primitive.Overlay className="overlay"/><Primitive.Content asChild>
      <motion.section className="dialog" initial={{ opacity: 0 }} animate={{ opacity: 1 }}
        transition={{ duration: reduce ? 0 : 0.16 }}>
        <Primitive.Title>{title}</Primitive.Title><Primitive.Description>{description}</Primitive.Description>
        {children}<Primitive.Close asChild><Button>Cerrar</Button></Primitive.Close>
      </motion.section>
    </Primitive.Content></Primitive.Portal>
  </Primitive.Root>;
}
// confirmation, cuando existe, debe coincidir exactamente antes de ejecutar confirm.
export function ConfirmDialog({ trigger, title, description, confirmation, label = 'Confirmar', confirm }: {
  trigger: string; title: string; description: string; confirmation?: string;
  label?: string; confirm: () => Promise<unknown>;
}) {
  return <Dialog trigger={trigger} title={title} description={description} variant="danger">
    <ActionForm label={label} variant="danger"
      fields={confirmation ? [{ name: 'confirmation', label: `Escribe «${confirmation}» para confirmar` }] : []}
      submit={async values => {
        if (confirmation && values.confirmation !== confirmation) throw new Error('La confirmación no coincide.');
        await confirm();
      }}/>
  </Dialog>;
}
export type ActionField = {
  name: string; label: string; value?: string; type?: string; options?: readonly string[];
  required?: boolean; help?: string;
};
export function ActionForm({ fields, label, submit, variant = 'primary' }: {
  fields: ActionField[]; label: string; submit: (values: Record<string, string>) => Promise<unknown>; variant?: Variant;
}) {
  const [message, setMessage] = useState('');
  const form = useForm({
    defaultValues: Object.fromEntries(fields.map(f => [f.name, f.value ?? ''])),
    onSubmit: async ({ value }) => {
      setMessage('');
      try { await submit(value); setMessage('Acción completada.'); }
      catch (error) { setMessage(error instanceof Error ? error.message : 'No se pudo completar la acción.'); }
    },
  });
  return <form className="action-form" onSubmit={event => { event.preventDefault(); void form.handleSubmit(); }}>
    <div className="fields">{fields.map(spec => <form.Field key={spec.name} name={spec.name}>
      {field => <Field label={spec.label} help={spec.help}>{props => spec.options ?
        <select {...props} value={field.state.value} onChange={e => field.handleChange(e.target.value)}>
          {spec.options.map(option => <option key={option}>{option}</option>)}
        </select> : <input {...props} type={spec.type ?? 'text'} required={spec.required !== false}
          value={field.state.value} onBlur={field.handleBlur} onChange={e => field.handleChange(e.target.value)}/>}
      </Field>}
    </form.Field>)}</div>
    <form.Subscribe selector={state => state.isSubmitting}>{pending =>
      <Button type="submit" variant={variant} loading={pending}>{label}</Button>}
    </form.Subscribe><p role="status">{message}</p>
  </form>;
}
// Único reloj visual; queda fuera de las regiones vivas.
export function UpdatedIndicator({ updatedAt }: { updatedAt: number }) {
  const now = useClock();
  return <p className="status">{updatedAt ? `Actualizado hace ${secondsSince(updatedAt, now)} s` : 'Sin actualización todavía.'}</p>;
}
// Indicador local sin reloj ni anuncios repetidos; QueryAnnouncements centraliza los cambios.
export function Status({ query }: {
  query: Pick<UseQueryResult, 'isPending' | 'isError' | 'error' | 'dataUpdatedAt' | 'isFetching'>;
}) {
  const message = query.isPending ? 'Cargando…' : query.isError ? `Error: ${query.error?.message}` :
    query.isFetching ? 'Actualizando…' : '';
  return <div className="status">{message}</div>;
}
// Montar una vez en el shell; solo anuncia datos distintos o errores, nunca el reloj.
export function QueryAnnouncements() {
  const cache = useQueryClient();
  const [message, setMessage] = useState('');
  useEffect(() => {
    const previous = new Map(cache.getQueryCache().getAll().map(query => [query.queryHash, query.state.data]));
    return cache.getQueryCache().subscribe(event => {
      if (event.type !== 'updated') return;
      if (event.action.type === 'error') {
        setMessage('Error al actualizar los datos.');
      } else if (event.action.type === 'success') {
        const data = event.query.state.data;
        if (previous.has(event.query.queryHash) && previous.get(event.query.queryHash) !== data) {
          setMessage('Los datos han cambiado.');
        }
        previous.set(event.query.queryHash, data);
      }
    });
  }, [cache]);
  return <div className="sr-only" aria-live="polite">{message}</div>;
}
export function Empty({ children = 'Todavía no hay datos.' }: { children?: React.ReactNode }) {
  return <p className="empty">{children}</p>;
}
export function RunButton({ children, run, variant = 'secondary' }: {
  children: React.ReactNode; run: () => Promise<unknown>; variant?: Variant;
}) {
  const [message, setMessage] = useState('');
  const [pending, setPending] = useState(false);
  return <><Button variant={variant} loading={pending} onClick={() => {
    setPending(true); setMessage('');
    void run().then(() => setMessage('Acción completada.'), error =>
      setMessage(error instanceof Error ? error.message : 'Error de conexión.')).finally(() => setPending(false));
  }}>{children}</Button><span role="status">{message}</span></>;
}
