-- ============================================================================
-- Seed 3 adaptive quizzes onto an EXISTING course.
--
--   * Each quiz is a 20-question pool (4 questions at each difficulty 1..5),
--     one distinct topic per quiz.
--   * serve_count = 10  -> students get a 10-question subset per attempt,
--     chosen near their proficiency target.
--   * attempts_allowed = 3 -> retakes let the adaptation kick in.
--
-- Requirements: run quiz_bank_migration.sql (or the updated schema.sql) first,
-- so serve_count / attempts_allowed / difficulty / topic columns exist.
--
-- >>> BEFORE RUNNING: set v_code to your course's course_code. <<<
-- Any student already enrolled in that course can take the quizzes.
-- Re-running this script is safe: it deletes its own previous seed first.
-- ============================================================================
DO $$
DECLARE
  v_code    TEXT   := 'CSC1106';                              -- CHANGE ME
  v_topics  TEXT[] := ARRAY['Variables', 'Loops', 'Functions']; -- one per quiz
  v_course  INT;
  v_creator INT;
  v_quiz    INT;
  v_q       INT;
  v_topic   TEXT;
  d         INT;
  k         INT;
  v_seq     INT;
BEGIN
  -- Resolve the course and the user id of its lecturer (quizzes.created_by).
  SELECT c.id, l.user_id
    INTO v_course, v_creator
    FROM courses c
    JOIN lecturers l ON l.id = c.lecturer_id
   WHERE c.course_code = v_code;

  IF v_course IS NULL THEN
    RAISE EXCEPTION 'Course % not found - set v_code to an existing course_code.', v_code;
  END IF;

  -- Clean any previous run of this seed for an idempotent re-seed.
  DELETE FROM quizzes
   WHERE course_id = v_course
     AND title LIKE 'Adaptive Demo:%';

  FOREACH v_topic IN ARRAY v_topics LOOP
    INSERT INTO quizzes
        (course_id, created_by, title, description,
         open_at, close_at, total_marks, serve_count, attempts_allowed)
    VALUES
        (v_course, v_creator,
         'Adaptive Demo: ' || v_topic,
         'Auto-seeded adaptive quiz on ' || v_topic ||
           ' - 20-question pool, serves 10 per attempt, 3 attempts allowed.',
         NOW() - INTERVAL '1 day',
         NOW() + INTERVAL '30 days',
         20, 10, 3)
    RETURNING id INTO v_quiz;

    -- 20 questions: 4 at each difficulty 1..5, each a 4-option MCQ.
    v_seq := 0;
    FOR d IN 1..5 LOOP
      FOR k IN 1..4 LOOP
        v_seq := v_seq + 1;
        INSERT INTO quiz_questions
            (quiz_id, question_text, question_type, marks, difficulty, topic)
        VALUES
            (v_quiz,
             format('%s Q%s (difficulty %s): choose the correct option.', v_topic, v_seq, d),
             'multiple_choice', 1, d, v_topic)
        RETURNING id INTO v_q;

        INSERT INTO quiz_options (question_id, option_text, is_correct) VALUES
            (v_q, format('%s answer %s - correct', v_topic, v_seq), TRUE),
            (v_q, 'Distractor A', FALSE),
            (v_q, 'Distractor B', FALSE),
            (v_q, 'Distractor C', FALSE);
      END LOOP;
    END LOOP;

    RAISE NOTICE 'Seeded quiz "Adaptive Demo: %" (id %) with 20 questions.', v_topic, v_quiz;
  END LOOP;
END $$;
