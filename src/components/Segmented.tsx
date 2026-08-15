import { motion } from "motion/react";
import { useId } from "react";

interface Option<T extends string> {
  label: string;
  value: T;
}

interface SegmentedProps<T extends string> {
  value: T;
  options: Option<T>[];
  onChange: (value: T) => void;
}

export function Segmented<T extends string>({ value, options, onChange }: SegmentedProps<T>) {
  const id = useId();
  return (
    <div className="segmented" role="radiogroup">
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          role="radio"
          aria-checked={value === option.value}
          data-active={value === option.value}
          onClick={() => onChange(option.value)}
        >
          {value === option.value && (
            <motion.span
              className="seg-thumb"
              layoutId={`seg-${id}`}
              transition={{ type: "spring", stiffness: 500, damping: 38 }}
            />
          )}
          <span>{option.label}</span>
        </button>
      ))}
    </div>
  );
}
