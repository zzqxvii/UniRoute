import { useState, useRef, useEffect, useCallback } from 'react';

interface TagInputProps {
  value: string[];
  onChange: (value: string[]) => void;
  suggestions?: string[];
  placeholder?: string;
  allowCustom?: boolean;
  disabled?: boolean;
}

export function TagInput({
  value,
  onChange,
  suggestions = [],
  placeholder = '输入后按回车添加...',
  allowCustom = true,
  disabled = false,
}: TagInputProps) {
  const [input, setInput] = useState('');
  const [showDropdown, setShowDropdown] = useState(false);
  const [highlightedIndex, setHighlightedIndex] = useState(0);
  const containerRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // 过滤建议列表
  const filteredSuggestions = suggestions.filter(
    (s) => !value.includes(s) && s.toLowerCase().includes(input.toLowerCase())
  );

  // 点击外部关闭下拉
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setShowDropdown(false);
      }
    };
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  // 重置高亮
  useEffect(() => {
    setHighlightedIndex(0);
  }, [input, showDropdown]);

  const addTag = useCallback((tag: string) => {
    const trimmed = tag.trim();
    if (trimmed && !value.includes(trimmed)) {
      onChange([...value, trimmed]);
      setInput('');
      setShowDropdown(false);
    }
  }, [value, onChange]);

  const removeTag = useCallback((tag: string) => {
    onChange(value.filter((t) => t !== tag));
  }, [value, onChange]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (disabled) return;

    switch (e.key) {
      case 'Enter':
        e.preventDefault();
        if (showDropdown && filteredSuggestions.length > 0) {
          addTag(filteredSuggestions[highlightedIndex]);
        } else if (allowCustom && input.trim()) {
          addTag(input);
        }
        break;
      case 'ArrowDown':
        e.preventDefault();
        if (showDropdown && highlightedIndex < filteredSuggestions.length - 1) {
          setHighlightedIndex(highlightedIndex + 1);
        }
        break;
      case 'ArrowUp':
        e.preventDefault();
        if (showDropdown && highlightedIndex > 0) {
          setHighlightedIndex(highlightedIndex - 1);
        }
        break;
      case 'Escape':
        setShowDropdown(false);
        break;
      case 'Backspace':
        if (input === '' && value.length > 0) {
          removeTag(value[value.length - 1]);
        }
        break;
    }
  };

  return (
    <div ref={containerRef} className="relative">
      <div
        className={`tag-input-container ${disabled ? 'opacity-50 cursor-not-allowed' : ''}`}
        onClick={() => !disabled && inputRef.current?.focus()}
      >
        {value.map((tag) => (
          <span key={tag} className="tag-item">
            {tag}
            {!disabled && (
              <span
                className="tag-remove"
                onClick={(e) => {
                  e.stopPropagation();
                  removeTag(tag);
                }}
              >
                <svg className="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
                </svg>
              </span>
            )}
          </span>
        ))}
        <input
          ref={inputRef}
          type="text"
          value={input}
          onChange={(e) => {
            setInput(e.target.value);
            setShowDropdown(true);
          }}
          onFocus={() => setShowDropdown(true)}
          onKeyDown={handleKeyDown}
          placeholder={value.length === 0 ? placeholder : ''}
          disabled={disabled}
          className="tag-input"
        />
      </div>

      {showDropdown && filteredSuggestions.length > 0 && (
        <div className="tag-dropdown">
          {filteredSuggestions.map((suggestion, idx) => (
            <div
              key={suggestion}
              className={`tag-dropdown-item ${idx === highlightedIndex ? 'active' : ''}`}
              onClick={() => addTag(suggestion)}
              onMouseEnter={() => setHighlightedIndex(idx)}
            >
              <span className="font-mono">{suggestion}</span>
            </div>
          ))}
        </div>
      )}

      {input && !allowCustom && filteredSuggestions.length === 0 && (
        <div className="tag-dropdown">
          <div className="px-3 py-2 text-sm text-gray-500 dark:text-gray-400 italic">
            无匹配建议
          </div>
        </div>
      )}
    </div>
  );
}
