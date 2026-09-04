import { useRef, type InputHTMLAttributes } from "react";
import { Icon } from "./Icon";
import { t } from "../i18n";

type NumberFieldProps = Omit<InputHTMLAttributes<HTMLInputElement>, "type" | "value" | "onChange"> & {
  value: string | number;
  onValueChange: (value: string) => void;
  onStepCommit?: (value: string) => void;
};

export function NumberField({ value, onValueChange, onStepCommit, className = "", style, ...inputProps }: NumberFieldProps) {
  const inputRef = useRef<HTMLInputElement | null>(null);

  function step(direction: 1 | -1) {
    const input = inputRef.current;
    if (!input) return;
    if (direction > 0) input.stepUp();
    else input.stepDown();
    onValueChange(input.value);
    onStepCommit?.(input.value);
  }

  return (
    <span className="number-field" style={style}>
      <input
        {...inputProps}
        ref={inputRef}
        className={"field number-field__input " + className}
        type="number"
        value={value}
        onChange={(event) => onValueChange(event.target.value)}
      />
      <span className="number-field__steppers">
        <button type="button" tabIndex={-1} aria-label={t("Увеличить значение")} onMouseDown={(event) => event.preventDefault()} onClick={() => step(1)}>
          <Icon name="chev-down" size={9}/>
        </button>
        <button type="button" tabIndex={-1} aria-label={t("Уменьшить значение")} onMouseDown={(event) => event.preventDefault()} onClick={() => step(-1)}>
          <Icon name="chev-down" size={9}/>
        </button>
      </span>
    </span>
  );
}
