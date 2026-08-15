interface SliderProps {
  min: number;
  max: number;
  step?: number;
  value: number;
  onChange: (value: number) => void;
}

export function Slider({ min, max, step, value, onChange }: SliderProps) {
  const fill = ((Math.min(Math.max(value, min), max) - min) / (max - min)) * 100;
  return (
    <input
      type="range"
      min={min}
      max={max}
      step={step}
      value={value}
      style={{ "--fill": `${fill}%` } as React.CSSProperties}
      onChange={e => onChange(Number(e.target.value))}
    />
  );
}
