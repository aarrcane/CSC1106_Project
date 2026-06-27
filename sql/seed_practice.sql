-- ============================================================================
-- Self-contained: ensure the practice schema exists first (same as
-- practice_migration.sql). Safe to run repeatedly.
-- ============================================================================
ALTER TABLE quizzes ADD COLUMN IF NOT EXISTS is_practice BOOLEAN NOT NULL DEFAULT FALSE;

CREATE TABLE IF NOT EXISTS student_practice_proficiency (
    id             SERIAL PRIMARY KEY,
    student_id     INT NOT NULL REFERENCES students(id) ON DELETE CASCADE,
    course_id      INT NOT NULL REFERENCES courses(id)  ON DELETE CASCADE,
    topic          VARCHAR(120) NOT NULL,
    proficiency    NUMERIC(4,3) NOT NULL DEFAULT 0.5,
    answered_count INT NOT NULL DEFAULT 0,
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (student_id, course_id, topic)
);

CREATE TABLE IF NOT EXISTS practice_attempt_proficiency (
    attempt_id  INT NOT NULL REFERENCES quiz_attempts(id) ON DELETE CASCADE,
    topic       VARCHAR(120) NOT NULL,
    prof_before NUMERIC(4,3) NOT NULL,
    prof_after  NUMERIC(4,3) NOT NULL,
    PRIMARY KEY (attempt_id, topic)
);

-- ============================================================================
-- seed_practice.sql
-- Seeds ONE adaptive practice quiz with a 15-question bank (3 topics x
-- difficulties 1..5) for the CSC1106 "Web Programming" course.
-- Run AFTER practice_migration.sql (or schema.sql) and seed_demo.sql.
-- Idempotent: re-running replaces the practice quiz cleanly.
-- ============================================================================
DO $$
DECLARE
  v_course  INT;
  v_creator INT;
  v_quiz    INT;
  qq        INT;
BEGIN
  SELECT id INTO v_course FROM courses WHERE course_code = 'CSC1106' LIMIT 1;
  IF v_course IS NULL THEN
    RAISE NOTICE 'CSC1106 not found - run seed_demo.sql first. Skipping.';
    RETURN;
  END IF;

  -- Course owner's user id (quizzes.created_by references users.id).
  SELECT l.user_id INTO v_creator
    FROM lecturers l
    JOIN courses c ON c.lecturer_id = l.id
   WHERE c.id = v_course;

  -- Clean any prior copy of this practice quiz (cascades to questions/options/attempts).
  DELETE FROM quizzes
   WHERE course_id = v_course AND is_practice = TRUE
     AND title = 'Practice - Web Fundamentals';

  INSERT INTO quizzes
      (course_id, created_by, title, description, open_at, close_at,
       total_marks, serve_count, attempts_allowed, is_practice)
  VALUES
      (v_course, v_creator,
       'Practice - Web Fundamentals',
       'Adaptive practice across HTML, CSS and JavaScript. Difficulty matches your proficiency.',
       NOW() - INTERVAL '365 days', NOW() + INTERVAL '3650 days',
       12, 6, 1, TRUE)
  RETURNING id INTO v_quiz;

  -- ── HTML (difficulty 1..5) ────────────────────────────────────────────────
  INSERT INTO quiz_questions (quiz_id, question_text, question_type, marks, difficulty, topic)
    VALUES (v_quiz, 'Which tag creates a hyperlink in HTML?', 'multiple_choice', 2, 1, 'HTML') RETURNING id INTO qq;
  INSERT INTO quiz_options (question_id, option_text, is_correct) VALUES (qq,'<a>',TRUE),(qq,'<link>',FALSE),(qq,'<href>',FALSE),(qq,'<url>',FALSE);

  INSERT INTO quiz_questions (quiz_id, question_text, question_type, marks, difficulty, topic)
    VALUES (v_quiz, 'The <section> element is a semantic element.', 'true_false', 2, 2, 'HTML') RETURNING id INTO qq;
  INSERT INTO quiz_options (question_id, option_text, is_correct) VALUES (qq,'True',TRUE),(qq,'False',FALSE);

  INSERT INTO quiz_questions (quiz_id, question_text, question_type, marks, difficulty, topic)
    VALUES (v_quiz, 'Which attribute provides alternative text for an image?', 'multiple_choice', 2, 3, 'HTML') RETURNING id INTO qq;
  INSERT INTO quiz_options (question_id, option_text, is_correct) VALUES (qq,'alt',TRUE),(qq,'title',FALSE),(qq,'src',FALSE),(qq,'caption',FALSE);

  INSERT INTO quiz_questions (quiz_id, question_text, question_type, marks, difficulty, topic)
    VALUES (v_quiz, 'Which input type groups radio buttons together?', 'multiple_choice', 2, 4, 'HTML') RETURNING id INTO qq;
  INSERT INTO quiz_options (question_id, option_text, is_correct) VALUES (qq,'Same name attribute',TRUE),(qq,'Same id attribute',FALSE),(qq,'Same class',FALSE),(qq,'Same value',FALSE);

  INSERT INTO quiz_questions (quiz_id, question_text, question_type, marks, difficulty, topic)
    VALUES (v_quiz, 'Which ARIA attribute identifies the role of an element for assistive tech?', 'multiple_choice', 2, 5, 'HTML') RETURNING id INTO qq;
  INSERT INTO quiz_options (question_id, option_text, is_correct) VALUES (qq,'role',TRUE),(qq,'aria-type',FALSE),(qq,'aria-tag',FALSE),(qq,'semantic',FALSE);

  -- ── CSS (difficulty 1..5) ─────────────────────────────────────────────────
  INSERT INTO quiz_questions (quiz_id, question_text, question_type, marks, difficulty, topic)
    VALUES (v_quiz, 'Which property changes text colour in CSS?', 'multiple_choice', 2, 1, 'CSS') RETURNING id INTO qq;
  INSERT INTO quiz_options (question_id, option_text, is_correct) VALUES (qq,'color',TRUE),(qq,'font-color',FALSE),(qq,'text-color',FALSE),(qq,'fill',FALSE);

  INSERT INTO quiz_questions (quiz_id, question_text, question_type, marks, difficulty, topic)
    VALUES (v_quiz, 'A class selector starts with a "." in CSS.', 'true_false', 2, 2, 'CSS') RETURNING id INTO qq;
  INSERT INTO quiz_options (question_id, option_text, is_correct) VALUES (qq,'True',TRUE),(qq,'False',FALSE);

  INSERT INTO quiz_questions (quiz_id, question_text, question_type, marks, difficulty, topic)
    VALUES (v_quiz, 'Which display value makes a flex container?', 'multiple_choice', 2, 3, 'CSS') RETURNING id INTO qq;
  INSERT INTO quiz_options (question_id, option_text, is_correct) VALUES (qq,'flex',TRUE),(qq,'block',FALSE),(qq,'inline',FALSE),(qq,'grid-flex',FALSE);

  INSERT INTO quiz_questions (quiz_id, question_text, question_type, marks, difficulty, topic)
    VALUES (v_quiz, 'Which selector has the highest specificity?', 'multiple_choice', 2, 4, 'CSS') RETURNING id INTO qq;
  INSERT INTO quiz_options (question_id, option_text, is_correct) VALUES (qq,'#id',TRUE),(qq,'.class',FALSE),(qq,'element',FALSE),(qq,'* universal',FALSE);

  INSERT INTO quiz_questions (quiz_id, question_text, question_type, marks, difficulty, topic)
    VALUES (v_quiz, 'In CSS, which unit is relative to the root element font size?', 'multiple_choice', 2, 5, 'CSS') RETURNING id INTO qq;
  INSERT INTO quiz_options (question_id, option_text, is_correct) VALUES (qq,'rem',TRUE),(qq,'em',FALSE),(qq,'px',FALSE),(qq,'pt',FALSE);

  -- ── JavaScript (difficulty 1..5) ──────────────────────────────────────────
  INSERT INTO quiz_questions (quiz_id, question_text, question_type, marks, difficulty, topic)
    VALUES (v_quiz, 'Which keyword declares a block-scoped variable?', 'multiple_choice', 2, 1, 'JavaScript') RETURNING id INTO qq;
  INSERT INTO quiz_options (question_id, option_text, is_correct) VALUES (qq,'let',TRUE),(qq,'var',FALSE),(qq,'def',FALSE),(qq,'const-only',FALSE);

  INSERT INTO quiz_questions (quiz_id, question_text, question_type, marks, difficulty, topic)
    VALUES (v_quiz, 'typeof null returns "object" in JavaScript.', 'true_false', 2, 2, 'JavaScript') RETURNING id INTO qq;
  INSERT INTO quiz_options (question_id, option_text, is_correct) VALUES (qq,'True',TRUE),(qq,'False',FALSE);

  INSERT INTO quiz_questions (quiz_id, question_text, question_type, marks, difficulty, topic)
    VALUES (v_quiz, 'Which method converts a JSON string into an object?', 'multiple_choice', 2, 3, 'JavaScript') RETURNING id INTO qq;
  INSERT INTO quiz_options (question_id, option_text, is_correct) VALUES (qq,'JSON.parse()',TRUE),(qq,'JSON.stringify()',FALSE),(qq,'JSON.toObject()',FALSE),(qq,'parseJSON()',FALSE);

  INSERT INTO quiz_questions (quiz_id, question_text, question_type, marks, difficulty, topic)
    VALUES (v_quiz, 'What does the === operator check?', 'multiple_choice', 2, 4, 'JavaScript') RETURNING id INTO qq;
  INSERT INTO quiz_options (question_id, option_text, is_correct) VALUES (qq,'Value and type equality',TRUE),(qq,'Value only',FALSE),(qq,'Reference only',FALSE),(qq,'Assignment',FALSE);

  INSERT INTO quiz_questions (quiz_id, question_text, question_type, marks, difficulty, topic)
    VALUES (v_quiz, 'What is the output type of an async function call?', 'multiple_choice', 2, 5, 'JavaScript') RETURNING id INTO qq;
  INSERT INTO quiz_options (question_id, option_text, is_correct) VALUES (qq,'A Promise',TRUE),(qq,'The resolved value',FALSE),(qq,'undefined',FALSE),(qq,'A callback',FALSE);

  RAISE NOTICE 'Seeded practice quiz id % for CSC1106 (15 questions).', v_quiz;
END $$;
