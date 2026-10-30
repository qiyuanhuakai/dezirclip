"""AST-loaded click planning checks; no native module import or input execution."""
import ast
import json
from pathlib import Path
import unittest


SOURCE = Path(__file__).with_name('native-windows-surface-target.py')
FUNCTIONS = {'require', 'absolute_mouse_point', 'alternate_click_candidates',
             'plan_alternate_click', 'send_planned_click'}
TREE = ast.parse(SOURCE.read_text(encoding='utf-8'))
DEFINITIONS = [node for node in TREE.body if isinstance(node, ast.FunctionDef) and node.name in FUNCTIONS]
if len(DEFINITIONS) != len(FUNCTIONS) or {node.name for node in DEFINITIONS} != FUNCTIONS:
    raise RuntimeError('The exact pure click-planning functions are required')
NAMESPACE = {'json': json}
exec(compile(ast.Module(body=DEFINITIONS, type_ignores=[]), str(SOURCE), 'exec'), NAMESPACE)
CANDIDATES = NAMESPACE['alternate_click_candidates']
PLAN = NAMESPACE['plan_alternate_click']
SEND = NAMESPACE['send_planned_click']


class ClickPlanTests(unittest.TestCase):
    def make_plan(self, hits):
        converted, inspected = [], []
        coordinates = [(-1792, 421), (-1248, 421), (-1520, 421)]
        metadata = {point: hit for point, hit in zip(coordinates, hits)}

        def to_screen(point):
            converted.append(list(point))
            return [point[0] - 1800, point[1] + 400]

        def inspect(point):
            inspected.append(list(point))
            return dict(metadata[tuple(point)])

        plan = PLAN((0, 0, 560, 42), (-1920, 0, 3840, 1080), 20, to_screen, inspect)
        self.assertEqual(plan['client_rect'], [0, 0, 560, 42])
        self.assertEqual(plan['virtual_desktop'], [-1920, 0, 3840, 1080])
        self.assertEqual([probe['id'] for probe in plan['probes']], ['left', 'right', 'center'])
        self.assertEqual(converted, [[8, 21], [552, 21], [280, 21]])
        self.assertEqual(inspected, [list(point) for point in coordinates])
        self.assertEqual([probe['screen_point'] for probe in plan['probes']], inspected)
        self.assertTrue(all(probe['in_virtual_desktop'] is True for probe in plan['probes']))
        self.assertIsInstance(plan['scope'], str)
        self.assertTrue(plan['scope'])
        return plan, inspected

    def test_candidates_are_predeclared_ordered_insets_and_center(self):
        self.assertEqual(CANDIDATES((0, 0, 560, 42)), [
            {'id': 'left', 'client_point': [8, 21]},
            {'id': 'right', 'client_point': [552, 21]},
            {'id': 'center', 'client_point': [280, 21]},
        ])
        self.assertEqual(CANDIDATES((0, 0, 17, 3)), [
            {'id': 'left', 'client_point': [8, 1]},
            {'id': 'right', 'client_point': [9, 1]},
        ])

    def test_invalid_client_bounds_are_rejected(self):
        for rect in ((1, 0, 560, 42), (0, 1, 560, 42), (0, 0, 16, 42), (0, 0, 560, 2)):
            with self.subTest(rect=rect), self.assertRaises(RuntimeError):
                CANDIDATES(rect)

    def test_clear_left_is_selected_after_all_three_readonly_probes(self):
        plan, inspected = self.make_plan([{'hwnd': 20, 'exists': True}] * 3)
        self.assertEqual(plan['selected_index'], 0)
        self.assertEqual([probe['eligible'] for probe in plan['probes']], [True, True, True])
        self.assertEqual(len(inspected), 3)

    def test_covered_left_selects_clear_right_before_clear_center(self):
        plan, _ = self.make_plan([{'hwnd': 99, 'exists': True}, {'hwnd': 20, 'exists': True},
                                 {'hwnd': 20, 'exists': True}])
        self.assertEqual(plan['selected_index'], 1)
        self.assertEqual([probe['eligible'] for probe in plan['probes']], [False, True, True])

    def test_center_is_last_option_and_nonexistent_matching_handle_is_ineligible(self):
        plan, _ = self.make_plan([{'hwnd': 20, 'exists': False}, {'hwnd': 99, 'exists': True},
                                 {'hwnd': 20, 'exists': True}])
        self.assertEqual(plan['selected_index'], 2)
        self.assertEqual([probe['eligible'] for probe in plan['probes']], [False, False, True])

    def test_all_covered_records_all_probes_and_sends_nothing(self):
        plan, inspected = self.make_plan([{'hwnd': 99, 'exists': True}] * 3)
        self.assertIsNone(plan['selected_index'])
        self.assertEqual([probe['eligible'] for probe in plan['probes']], [False, False, False])
        rechecks, sends = [], []
        with self.assertRaises(RuntimeError) as raised:
            SEND(plan, lambda probe: rechecks.append(probe), lambda point: sends.append(point))
        self.assertEqual((len(inspected), rechecks, sends), (3, [], []))
        self.assertIn('selected_index', str(raised.exception))
        self.assertIn('probes', str(raised.exception))

    def test_off_desktop_probe_is_not_inspected_and_duplicate_candidate_is_removed(self):
        converted, inspected = [], []

        def to_screen(point):
            converted.append(list(point))
            return list(point)

        def inspect(point):
            inspected.append(list(point))
            return {'hwnd': 20, 'exists': True}

        plan = PLAN((0, 0, 17, 3), (9, 0, 1, 3), 20, to_screen, inspect)
        self.assertEqual(converted, [[8, 1], [9, 1]])
        self.assertEqual(inspected, [[9, 1]])
        self.assertEqual([probe['id'] for probe in plan['probes']], ['left', 'right'])
        outside, inside = plan['probes']
        self.assertEqual(outside['client_point'], [8, 1])
        self.assertEqual(outside['screen_point'], [8, 1])
        self.assertIs(outside['in_virtual_desktop'], False)
        self.assertIsNone(outside['absolute_point'])
        self.assertIsNone(outside['hit'])
        self.assertIs(outside['eligible'], False)
        self.assertIs(inside['in_virtual_desktop'], True)
        self.assertIs(inside['eligible'], True)
        self.assertIsNotNone(inside['absolute_point'])
        self.assertEqual(len(inside['absolute_point']), 2)
        self.assertEqual(plan['selected_index'], 1)

    def test_failed_selected_recheck_never_sends_or_selects_another_point(self):
        plan, inspected = self.make_plan([{'hwnd': 99, 'exists': True}, {'hwnd': 20, 'exists': True},
                                         {'hwnd': 20, 'exists': True}])
        rechecks, sends = [], []
        rejection = {'unchanged': False, 'reason': 'synthetic selected target changed'}

        def recheck(probe):
            rechecks.append(json.loads(json.dumps(probe)))
            return dict(rejection)

        with self.assertRaises(RuntimeError) as raised:
            SEND(plan, recheck, lambda point: sends.append(point))
        self.assertEqual(len(rechecks), 1)
        self.assertEqual(rechecks[0]['id'], 'right')
        self.assertEqual((plan['selected_index'], len(inspected), sends), (1, 3, []))
        self.assertEqual(plan['recheck'], rejection)
        self.assertIn('synthetic selected target changed', str(raised.exception))

    def test_planning_callback_failure_retains_prior_and_failed_probe_without_continuing(self):
        for failing in ('to_screen', 'inspect'):
            with self.subTest(failing=failing):
                converted, inspected = [], []

                def to_screen(point):
                    converted.append(list(point))
                    if failing == 'to_screen' and len(converted) == 2:
                        raise OSError('synthetic screen conversion failed')
                    return list(point)

                def inspect(point):
                    inspected.append(list(point))
                    if failing == 'inspect' and len(inspected) == 2:
                        raise OSError('synthetic hit inspection failed')
                    return {'hwnd': 20, 'exists': True}

                with self.assertRaisesRegex(RuntimeError, 'planning failed') as raised:
                    PLAN((0, 0, 560, 42), (0, 0, 560, 42), 20, to_screen, inspect)
                recorded = json.loads(str(raised.exception).split(': ', 1)[1])
                self.assertEqual([probe['id'] for probe in recorded['probes']], ['left', 'right'])
                self.assertIs(recorded['probes'][0]['eligible'], True)
                self.assertIs(recorded['probes'][1]['eligible'], False)
                self.assertIn('synthetic', recorded['probes'][1]['error'])
                self.assertEqual(recorded['selected_index'], 0)
                self.assertEqual(len(converted), 2)
                self.assertEqual(len(inspected), 1 if failing == 'to_screen' else 2)

    def test_selected_point_is_rechecked_once_and_sent_in_one_batch(self):
        plan, inspected = self.make_plan([{'hwnd': 99, 'exists': True}, {'hwnd': 20, 'exists': True},
                                         {'hwnd': 20, 'exists': True}])
        rechecks, sends = [], []

        def recheck(probe):
            rechecks.append(probe['id'])
            return {'unchanged': True, 'checked': 'synthetic original target'}

        def send(point):
            sends.append(list(point))
            return 3

        self.assertEqual(SEND(plan, recheck, send), 3)
        self.assertEqual((rechecks, len(inspected)), (['right'], 3))
        self.assertEqual(sends, [list(plan['probes'][1]['absolute_point'])])
        self.assertEqual(plan['sent'], 3)
        self.assertIs(plan['recheck']['unchanged'], True)

    def test_partial_or_invalid_input_count_is_preserved_without_retry(self):
        for count in (0, 1, 2, 4):
            with self.subTest(count=count):
                plan, inspected = self.make_plan([{'hwnd': 20, 'exists': True}] * 3)
                rechecks, sends = [], []

                def recheck(probe):
                    rechecks.append(probe['id'])
                    return {'unchanged': True}

                def send(point):
                    sends.append(list(point))
                    return count

                with self.assertRaisesRegex(RuntimeError, 'incomplete') as raised:
                    SEND(plan, recheck, send)
                self.assertEqual((rechecks, len(inspected), len(sends)), (['left'], 3, 1))
                self.assertEqual(plan['sent'], count)
                self.assertIn('sent', str(raised.exception))


if __name__ == '__main__':
    unittest.main()
