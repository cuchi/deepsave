interface Props {
  bank?: string | null
  className?: string
}

const LOGO_EXT: Record<string, string> = {
  c6: 'png',
  nubank: 'svg',
  caixa: 'svg',
}

export default function BankLogo({ bank, className = 'h-5 w-5' }: Props) {
  if (!bank) {
    return <span className={`${className} inline-block shrink-0 rounded bg-zinc-800`} />
  }
  const ext = LOGO_EXT[bank] ?? 'svg'
  return (
    <img
      src={`/logos/${bank}.${ext}`}
      alt={bank}
      className={`${className} shrink-0 object-contain`}
    />
  )
}
