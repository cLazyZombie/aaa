// 보고서 Markdown(차이 목록 R1). herdr-web-ui `src/components/Markdown.tsx`(aa624cb)의 렌더를 옮기고, 파서는
// `src/lib/markdown.ts`·`markdownHtml.ts`를 그대로 쓴다(herdr/markdown.ts). HTML은 해석하지 않는다(parseMarkdown html=false).
// 참고에서 뺀 것: 수식(KaTeX)은 원문 그대로, mermaid는 code block, 그림은 참고의 「그림을 그리지 않는 곳」처럼 대체 글,
// 파일 열기는 aaa에 앱 안 viewer가 아직 없어 참고 「열 수 없는 곳」 모양(이름 뒤 경로). 문법 강조는 없다(글자 그대로).
// 문구는 참고 i18n.ko.ts 그대로다.
import { memo, useId, useMemo, useState, type ReactNode } from "react";
import { Check, Copy } from "lucide-react";
import { foldCode, parseMarkdown, type InlineNode, type ListBlock, type MarkdownBlock } from "./herdr/markdown.ts";
import "./herdr/Markdown.css";

/** `interactive`가 false면 링크·파일 이름 안이다: 누를 것을 겹치지 않는다(참고 Inline) */
function Inline({ nodes, interactive = true }: { nodes: InlineNode[]; interactive?: boolean }) {
  return (
    <>
      {nodes.map((node, index) => {
        const key = `${node.type}-${index}`;
        switch (node.type) {
          case "text":
            return <span key={key}>{node.value}</span>;
          case "code":
            // agent는 주소를 backtick에 넣곤 한다: 눈에는 code로 두고 열린다
            if (interactive && /^https?:\/\/\S+$/i.test(node.value))
              return (
                <a key={key} className="markdown-code-link" href={node.value} target="_blank" rel="noopener noreferrer">
                  <code>{node.value}</code>
                </a>
              );
            return <code key={key}>{node.value}</code>;
          case "math":
            return <span key={key}>{`\\(${node.value}\\)`}</span>;
          case "image":
            return (
              <span key={key} data-markdown-copy={`![${node.alt}](${node.src})`}>
                <Inline nodes={node.fallback} interactive={interactive} />
              </span>
            );
          case "strong":
            return (
              <strong key={key}>
                <Inline nodes={node.children} interactive={interactive} />
              </strong>
            );
          case "em":
            return (
              <em key={key}>
                <Inline nodes={node.children} interactive={interactive} />
              </em>
            );
          case "del":
            return (
              <del key={key}>
                <Inline nodes={node.children} interactive={interactive} />
              </del>
            );
          case "link":
            return (
              <a key={key} href={node.href} target="_blank" rel="noopener noreferrer">
                <Inline nodes={node.children} interactive={false} />
              </a>
            );
          case "file":
            // 열 곳이 없으면 이름 뒤에 경로를 보인다(참고: Codex 터미널처럼)
            return (
              <span key={key}>
                <Inline nodes={node.children} interactive={false} /> (<code>{node.path}</code>)
              </span>
            );
        }
      })}
    </>
  );
}

/** 목록, 항목 안의 목록·표. 할 일 항목은 상자가 글머리표 자리에(참고 List) */
function List({ block }: { block: ListBlock }) {
  const Tag = block.ordered ? "ol" : "ul";
  const id = useId();
  return (
    <Tag className="markdown-list" start={block.ordered ? block.start : undefined}>
      {block.items.map((item, index) => (
        <li key={index} className={item.checked === undefined ? undefined : "markdown-task"}>
          {item.checked !== undefined && (
            <span
              className="markdown-task-box"
              role="checkbox"
              aria-checked={item.checked}
              aria-disabled="true"
              aria-labelledby={item.content.length === 0 ? undefined : `${id}-${index}`}
              aria-label={item.content.length === 0 ? "빈 할 일" : undefined}
            >
              {item.checked && <Check aria-hidden="true" />}
            </span>
          )}
          {item.checked === undefined ? (
            <Inline nodes={item.content} />
          ) : (
            <span id={`${id}-${index}`}>
              <Inline nodes={item.content} />
            </span>
          )}
          {item.blocks !== undefined && <Blocks blocks={item.blocks} />}
        </li>
      ))}
    </Tag>
  );
}

/** code block: 머리에 언어와 복사, 긴 것은 접고 「n줄 모두 보기」(참고 CodeBlock, 안쪽 스크롤 없음) */
function CodeBlock({ language, value }: { language: string; value: string }) {
  const [copied, setCopied] = useState(false);
  const [copyFailed, setCopyFailed] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const fold = useMemo(() => foldCode(value), [value]);
  const copy = async () => {
    let ok = false;
    try {
      await navigator.clipboard.writeText(value);
      ok = true;
    } catch {
      // clipboard API가 없거나 브라우저가 거절했다
    }
    setCopied(ok);
    setCopyFailed(!ok);
    if (ok) window.setTimeout(() => setCopied(false), 1500);
  };
  return (
    <div className="markdown-code" data-language={language}>
      <div className="markdown-code-header">
        <span>{language || "text"}</span>
        <button type="button" className="icon-button markdown-code-copy" onClick={() => void copy()} aria-label={copied ? "코드 복사됨" : "코드 복사"}>
          {copied ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
        </button>
      </div>
      {copyFailed && (
        <p className="markdown-code-error" role="alert">
          복사하지 못했습니다. 텍스트를 선택해 직접 복사하세요.
        </p>
      )}
      <pre className="hl-code">
        <code>{fold !== null && !expanded ? fold.head : value}</code>
      </pre>
      {fold !== null && (
        <button type="button" className="markdown-code-more" aria-expanded={expanded} onClick={() => setExpanded(!expanded)}>
          {expanded ? "접기" : `${fold.lines}줄 모두 보기`}
        </button>
      )}
    </div>
  );
}

function Blocks({ blocks }: { blocks: MarkdownBlock[] }) {
  return (
    <>
      {blocks.map((block, index): ReactNode => {
        const key = `${block.type}-${index}`;
        switch (block.type) {
          case "heading":
            // agent 제목은 몇 단계든 h3다: 앱의 h1·h2가 위 개요로 남는다. 단계는 class로(참고 Blocks)
            return (
              <h3 key={key} data-markdown-heading={block.level} className={`markdown-h${block.level}${block.align ? " markdown-center" : ""}`}>
                <Inline nodes={block.content} />
              </h3>
            );
          case "paragraph":
            return (
              <p key={key} className={block.align ? "markdown-center" : undefined}>
                {block.lines.map((line, lineIndex) => (
                  <span key={lineIndex}>
                    <Inline nodes={line} />
                    {lineIndex < block.lines.length - 1 && <br />}
                  </span>
                ))}
              </p>
            );
          case "list":
            return <List key={key} block={block} />;
          case "blockquote":
            return (
              <blockquote key={key}>
                <Blocks blocks={block.blocks} />
              </blockquote>
            );
          case "code":
            return <CodeBlock key={key} language={block.language} value={block.value} />;
          case "math":
            return <p key={key}>{`\\[${block.value}\\]`}</p>;
          case "hr":
            return <hr key={key} />;
          case "table":
            return (
              <div className="markdown-table-wrap" key={key}>
                <table>
                  <thead>
                    <tr>
                      {block.header.map((cell, cellIndex) => (
                        <th key={cellIndex}>
                          <Inline nodes={cell} />
                        </th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {block.rows.map((row, rowIndex) => (
                      <tr key={rowIndex}>
                        {row.map((cell, cellIndex) => (
                          <td key={cellIndex}>
                            <Inline nodes={cell} />
                          </td>
                        ))}
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            );
        }
      })}
    </>
  );
}

/** 그린 Markdown. props가 원시 값이라 memo가 부모의 다시 그리기를 건너뛴다(참고 Markdown) */
export const Markdown = memo(function Markdown({ children, className }: { children: string; className?: string }) {
  const blocks = useMemo(() => parseMarkdown(children), [children]);
  return (
    <div className={className === undefined ? "markdown" : `markdown ${className}`}>
      <Blocks blocks={blocks} />
    </div>
  );
});
