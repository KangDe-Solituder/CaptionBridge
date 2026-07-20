interface ToggleProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
}

export function Toggle({ checked, onChange, label }: ToggleProps) {
  return (
    <button
      className="toggle"
      data-on={checked}
      onClick={() => onChange(!checked)}
      type="button"
      aria-pressed={checked}
      aria-label={label}
    >
      <span />
    </button>
  );
}
